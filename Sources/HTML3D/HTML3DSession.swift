import Foundation
import JavaScriptCore
import Metal
import QuartzCore
import simd

enum HTML3DLayout {
    static let header = 160
    static let record = 40
    static let maxLights = 4

    static let view = 0
    static let projection = 16
    static let shadowViewProjection = 32
    static let camera = 48
    static let clear = 52
    static let fog = 56
    static let fogParams = 60
    static let ambient = 64
    static let hemiSky = 68
    static let hemiGround = 72
    static let hemiDirection = 76
    static let directionalColor = 80
    static let directionalDirection = 84
    static let shadow = 88
    static let shadowMapSize = 92
    static let lightCount = 93
    static let recordCount = 94
    static let pixelRatio = 95
    static let lights = 96

    static let recordGeometry = 0
    static let recordMaterial = 1
    static let recordStart = 2
    static let recordCount_ = 3
    static let recordFlags = 4
    static let recordInstances = 5
    static let recordInstanceCount = 6
    static let recordMatrix = 8
    static let recordColor = 24
    static let recordEmissive = 28
    static let recordSpecular = 32
    static let recordParams = 36
}

public struct HTML3DStats {
    public let frames: Int
    public let submitted: Int
    public let gpuSeconds: Double
    public let cpuSeconds: Double
    public let gpuTimed: Bool
    public let draws: Int
    public let triangles: Int
    public let memoryBytes: Int
}

struct HTML3DFrameUniforms {
    var view = matrix_identity_float4x4
    var projection = matrix_identity_float4x4
    var shadowViewProjection = matrix_identity_float4x4
    var cameraPosition = SIMD4<Float>(repeating: 0)
    var ambient = SIMD4<Float>(repeating: 0)
    var hemiSky = SIMD4<Float>(repeating: 0)
    var hemiGround = SIMD4<Float>(repeating: 0)
    var hemiDirection = SIMD4<Float>(repeating: 0)
    var directionalColor = SIMD4<Float>(repeating: 0)
    var directionalDirection = SIMD4<Float>(repeating: 0)
    var shadowParams = SIMD4<Float>(repeating: 0)
    var shadowSize = SIMD4<Float>(repeating: 0)
    var fogColor = SIMD4<Float>(repeating: 0)
    var fogParams = SIMD4<Float>(repeating: 0)
    var viewport = SIMD4<Float>(repeating: 0)
    var lightPosition = (SIMD4<Float>(repeating: 0), SIMD4<Float>(repeating: 0), SIMD4<Float>(repeating: 0), SIMD4<Float>(repeating: 0))
    var lightColor = (SIMD4<Float>(repeating: 0), SIMD4<Float>(repeating: 0), SIMD4<Float>(repeating: 0), SIMD4<Float>(repeating: 0))
    var lightDirection = (SIMD4<Float>(repeating: 0), SIMD4<Float>(repeating: 0), SIMD4<Float>(repeating: 0), SIMD4<Float>(repeating: 0))
    var lightCone = (SIMD4<Float>(repeating: 0), SIMD4<Float>(repeating: 0), SIMD4<Float>(repeating: 0), SIMD4<Float>(repeating: 0))
    var counts = SIMD4<Int32>(repeating: 0)
}

struct HTML3DDrawUniforms {
    var base = matrix_identity_float4x4
    var color = SIMD4<Float>(repeating: 1)
    var emissive = SIMD4<Float>(repeating: 0)
    var specular = SIMD4<Float>(repeating: 0)
    var params = SIMD4<Float>(repeating: 0)
    var layout = SIMD4<UInt32>(repeating: 0)
}

final class HTML3DGPU {
    static let shared: HTML3DGPU? = HTML3DGPU()

    static let sampleCount = 4
    static let colorFormat: MTLPixelFormat = .bgra8Unorm_srgb
    static let depthFormat: MTLPixelFormat = .depth32Float

    let device: MTLDevice
    let queue: MTLCommandQueue
    let dummyBuffer: MTLBuffer
    let dummyDepth: MTLTexture
    let shadowSampler: MTLSamplerState
    let memorylessTargets: Bool
    private let lock = NSLock()
    private var compiled: Pipelines?
    private var failed = false

    struct Pipelines {
        let mesh: MTLRenderPipelineState
        let meshBlend: MTLRenderPipelineState
        let points: MTLRenderPipelineState
        let pointsBlend: MTLRenderPipelineState
        let shadow: MTLRenderPipelineState
        let depthStates: [MTLDepthStencilState]
        let shadowDepth: MTLDepthStencilState
    }

    private init?() {
        guard let device = MTLCreateSystemDefaultDevice(), let queue = device.makeCommandQueue() else {
            return nil
        }
        let ones = [Float](repeating: 1, count: 16)
        guard let dummyBuffer = device.makeBuffer(bytes: ones, length: ones.count * 4, options: .storageModeShared) else {
            return nil
        }
        let depthDesc = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: HTML3DGPU.depthFormat, width: 1, height: 1, mipmapped: false)
        depthDesc.usage = [.shaderRead, .renderTarget]
        depthDesc.storageMode = .private
        guard let dummyDepth = device.makeTexture(descriptor: depthDesc) else {
            return nil
        }
        let samplerDesc = MTLSamplerDescriptor()
        samplerDesc.minFilter = .linear
        samplerDesc.magFilter = .linear
        samplerDesc.sAddressMode = .clampToEdge
        samplerDesc.tAddressMode = .clampToEdge
        samplerDesc.compareFunction = .lessEqual
        guard let shadowSampler = device.makeSamplerState(descriptor: samplerDesc) else {
            return nil
        }
        var memoryless = false
        if #available(macOS 11.0, *) {
            memoryless = device.supportsFamily(.apple7)
        }
        self.device = device
        self.queue = queue
        self.dummyBuffer = dummyBuffer
        self.dummyDepth = dummyDepth
        self.shadowSampler = shadowSampler
        self.memorylessTargets = memoryless
    }

    var pipelines: Pipelines? {
        lock.lock()
        defer { lock.unlock() }
        if compiled == nil, !failed {
            compiled = compile()
            failed = compiled == nil
        }
        return compiled
    }

    private func compile() -> Pipelines? {
        guard let library = try? device.makeLibrary(source: html3DShaderSource, options: nil),
              let vertex = library.makeFunction(name: "html3DVertex"),
              let fragment = library.makeFunction(name: "html3DFragment"),
              let pointVertex = library.makeFunction(name: "html3DPointVertex"),
              let pointFragment = library.makeFunction(name: "html3DPointFragment"),
              let shadowVertex = library.makeFunction(name: "html3DShadowVertex") else {
            return nil
        }
        func color(_ vertex: MTLFunction, _ fragment: MTLFunction, blend: Bool) -> MTLRenderPipelineState? {
            let desc = MTLRenderPipelineDescriptor()
            desc.vertexFunction = vertex
            desc.fragmentFunction = fragment
            desc.sampleCount = HTML3DGPU.sampleCount
            desc.colorAttachments[0].pixelFormat = HTML3DGPU.colorFormat
            desc.depthAttachmentPixelFormat = HTML3DGPU.depthFormat
            if blend {
                let attachment = desc.colorAttachments[0]!
                attachment.isBlendingEnabled = true
                attachment.rgbBlendOperation = .add
                attachment.alphaBlendOperation = .add
                attachment.sourceRGBBlendFactor = .sourceAlpha
                attachment.destinationRGBBlendFactor = .oneMinusSourceAlpha
                attachment.sourceAlphaBlendFactor = .one
                attachment.destinationAlphaBlendFactor = .oneMinusSourceAlpha
            }
            return try? device.makeRenderPipelineState(descriptor: desc)
        }
        let shadowDesc = MTLRenderPipelineDescriptor()
        shadowDesc.vertexFunction = shadowVertex
        shadowDesc.depthAttachmentPixelFormat = HTML3DGPU.depthFormat
        guard let mesh = color(vertex, fragment, blend: false),
              let meshBlend = color(vertex, fragment, blend: true),
              let points = color(pointVertex, pointFragment, blend: false),
              let pointsBlend = color(pointVertex, pointFragment, blend: true),
              let shadow = try? device.makeRenderPipelineState(descriptor: shadowDesc) else {
            return nil
        }
        var depthStates: [MTLDepthStencilState] = []
        for index in 0 ..< 4 {
            let desc = MTLDepthStencilDescriptor()
            desc.depthCompareFunction = index & 1 != 0 ? .lessEqual : .always
            desc.isDepthWriteEnabled = index & 2 != 0
            guard let state = device.makeDepthStencilState(descriptor: desc) else {
                return nil
            }
            depthStates.append(state)
        }
        let shadowDepthDesc = MTLDepthStencilDescriptor()
        shadowDepthDesc.depthCompareFunction = .lessEqual
        shadowDepthDesc.isDepthWriteEnabled = true
        guard let shadowDepth = device.makeDepthStencilState(descriptor: shadowDepthDesc) else {
            return nil
        }
        return Pipelines(mesh: mesh, meshBlend: meshBlend, points: points, pointsBlend: pointsBlend, shadow: shadow, depthStates: depthStates, shadowDepth: shadowDepth)
    }
}

public final class HTML3DSession {
    private struct Geometry {
        let positions: MTLBuffer
        let normals: MTLBuffer?
        let colors: MTLBuffer?
        let colorSize: UInt32
        let index: MTLBuffer?
        let indexType: MTLIndexType
        let vertexCount: Int
        let indexCount: Int
    }

    private struct InstanceSet {
        let matrices: MTLBuffer
        let colors: MTLBuffer?
        let count: Int
    }

    private struct InstanceSlots {
        var matrices: [MTLBuffer?] = Array(repeating: nil, count: 5)
        var colors: [MTLBuffer?] = Array(repeating: nil, count: 5)
        var next = 0
    }

    private struct BatchKey: Hashable {
        let geometry: Int
        let material: Int
        let flags: Int
        let start: Int
        let count: Int
    }

    private struct Batch {
        var geometry: Geometry
        var draw: HTML3DDrawUniforms
        var primitive: Int
        var materialFlags: Int
        var castShadow: Bool
        var start: Int
        var count: Int
        var matrices: [simd_float4x4]
        var instanceSet: InstanceSet?
        var instanceCount: Int
        var instanceOffset: Int
        var depth: Float
    }

    private static let maxFloats = 4 << 20
    private static let maxGeometryBytes = 64 << 20

    public let isSupported: Bool
    private let gpu: HTML3DGPU?
    private let lock = NSLock()
    private var geometries: [Int: Geometry] = [:]
    private var instanceSets: [Int: InstanceSet] = [:]
    private var instanceSlots: [Int: InstanceSlots] = [:]
    private var geometryBytes = 0
    private var instanceBytes = 0
    private var statFrames = 0
    private var statSubmitted = 0
    private var statGPUSeconds: Double = 0
    private var statCPUSeconds: Double = 0
    private var statDraws = 0
    private var statTriangles = 0
    private var statMemory = 0
    private var pending: [Float] = []
    private var pendingCount = 0
    private var hasPending = false
    private var renderScheduled = false
    private var layer: CAMetalLayer?
    private var viewportSize: CGSize = .zero
    private var viewportScale: CGFloat = 2
    private var radius: CGFloat = 0
    private var pixelRatioHint: Double = 2

    private let renderQueue = DispatchQueue(label: "HTML3D", qos: .userInteractive)
    private let inFlight = DispatchSemaphore(value: 3)
    private var working: [Float] = []
    private var ring: [MTLBuffer?] = [nil, nil, nil]
    private var ringIndex = 0
    private var colorTarget: MTLTexture?
    private var depthTarget: MTLTexture?
    private var shadowMap: MTLTexture?

    public init() {
        gpu = HTML3DGPU.shared
        isSupported = gpu != nil
        if let gpu = gpu {
            renderQueue.async {
                _ = gpu.pipelines
            }
        }
    }

    public var cornerRadius: CGFloat {
        lock.lock()
        defer { lock.unlock() }
        return radius
    }

    public var stats: HTML3DStats {
        var gpuTimed = false
        if #available(macOS 10.15, *) {
            gpuTimed = true
        }
        lock.lock()
        defer { lock.unlock() }
        return HTML3DStats(frames: statFrames, submitted: statSubmitted, gpuSeconds: statGPUSeconds, cpuSeconds: statCPUSeconds, gpuTimed: gpuTimed, draws: statDraws, triangles: statTriangles, memoryBytes: statMemory)
    }

    public func makeLayer() -> CAMetalLayer? {
        guard let gpu = gpu else {
            return nil
        }
        let layer = CAMetalLayer()
        layer.device = gpu.device
        layer.pixelFormat = HTML3DGPU.colorFormat
        layer.framebufferOnly = true
        layer.isOpaque = false
        layer.allowsNextDrawableTimeout = true
        layer.colorspace = CGColorSpace(name: CGColorSpace.sRGB)
        if #available(macOS 10.13.2, *) {
            layer.maximumDrawableCount = 3
        }
        layer.actions = ["bounds": NSNull(), "position": NSNull(), "frame": NSNull(), "contents": NSNull(), "cornerRadius": NSNull(), "hidden": NSNull()]
        return layer
    }

    public func attach(_ layer: CAMetalLayer, size: CGSize, scale: CGFloat) {
        lock.lock()
        self.layer = layer
        viewportSize = size
        viewportScale = scale
        lock.unlock()
    }

    public func detach(_ layer: CAMetalLayer) {
        lock.lock()
        if self.layer === layer {
            self.layer = nil
        }
        lock.unlock()
    }

    func install(in context: JSContext) -> JSValue {
        let native = JSValue(newObjectIn: context)!
        native.setObject(isSupported, forKeyedSubscript: "supported" as NSString)
        let size: @convention(block) () -> [Double] = { [weak self] in
            return self?.viewportInfo() ?? [0, 0, 2]
        }
        let configure: @convention(block) (Double) -> Void = { [weak self] radius in
            self?.configure(radius: radius)
        }
        let geometry: @convention(block) (Int, JSValue, JSValue, JSValue, Int, JSValue) -> Void = { [weak self] id, positions, normals, colors, colorSize, index in
            self?.defineGeometry(id: id, positions: positions, normals: normals, colors: colors, colorSize: colorSize, index: index)
        }
        let release: @convention(block) (Int) -> Void = { [weak self] id in
            self?.releaseGeometry(id: id)
        }
        let instances: @convention(block) (Int, JSValue, Int, JSValue) -> Void = { [weak self] id, matrices, count, colors in
            self?.defineInstances(id: id, matrices: matrices, count: count, colors: colors)
        }
        let releaseInstances: @convention(block) (Int) -> Void = { [weak self] id in
            self?.releaseInstances(id: id)
        }
        let frame: @convention(block) (JSValue, Int) -> Void = { [weak self] data, count in
            self?.submit(data: data, count: count)
        }
        native.setObject(size, forKeyedSubscript: "size" as NSString)
        native.setObject(configure, forKeyedSubscript: "configure" as NSString)
        native.setObject(geometry, forKeyedSubscript: "geometry" as NSString)
        native.setObject(release, forKeyedSubscript: "release" as NSString)
        native.setObject(instances, forKeyedSubscript: "instances" as NSString)
        native.setObject(releaseInstances, forKeyedSubscript: "releaseInstances" as NSString)
        native.setObject(frame, forKeyedSubscript: "frame" as NSString)
        return native
    }

    private func viewportInfo() -> [Double] {
        lock.lock()
        defer { lock.unlock() }
        return [Double(viewportSize.width), Double(viewportSize.height), Double(viewportScale)]
    }

    private func configure(radius: Double) {
        guard radius.isFinite else {
            return
        }
        lock.lock()
        self.radius = CGFloat(max(0, min(radius, 200)))
        lock.unlock()
    }

    private static func count(_ value: Float, upTo limit: Int) -> Int {
        guard value.isFinite, value > 0 else {
            return 0
        }
        return value >= Float(limit) ? limit : Int(value)
    }

    private static func bytes(of value: JSValue) -> (pointer: UnsafeMutableRawPointer, length: Int, type: JSTypedArrayType)? {
        guard let context = value.context?.jsGlobalContextRef, let ref = value.jsValueRef else {
            return nil
        }
        let type = JSValueGetTypedArrayType(context, ref, nil)
        if type == kJSTypedArrayTypeNone || type == kJSTypedArrayTypeArrayBuffer {
            return nil
        }
        guard let object = JSValueToObject(context, ref, nil), let pointer = JSObjectGetTypedArrayBytesPtr(context, object, nil) else {
            return nil
        }
        return (pointer, JSObjectGetTypedArrayByteLength(context, object, nil), type)
    }

    private func makeBuffer(_ value: JSValue, expected: JSTypedArrayType) -> MTLBuffer? {
        guard let gpu = gpu, let bytes = HTML3DSession.bytes(of: value), bytes.type == expected, bytes.length > 0 else {
            return nil
        }
        return gpu.device.makeBuffer(bytes: bytes.pointer, length: bytes.length, options: .storageModeShared)
    }

    private func defineGeometry(id: Int, positions: JSValue, normals: JSValue, colors: JSValue, colorSize: Int, index: JSValue) {
        guard let gpu = gpu, let position = makeBuffer(positions, expected: kJSTypedArrayTypeFloat32Array) else {
            return
        }
        let normal = makeBuffer(normals, expected: kJSTypedArrayTypeFloat32Array)
        let color = colorSize == 3 || colorSize == 4 ? makeBuffer(colors, expected: kJSTypedArrayTypeFloat32Array) : nil
        var indexBuffer: MTLBuffer?
        var indexType = MTLIndexType.uint16
        var indexCount = 0
        if let bytes = HTML3DSession.bytes(of: index), bytes.length > 0 {
            if bytes.type == kJSTypedArrayTypeUint16Array {
                indexBuffer = gpu.device.makeBuffer(bytes: bytes.pointer, length: bytes.length, options: .storageModeShared)
                indexCount = bytes.length / 2
            } else if bytes.type == kJSTypedArrayTypeUint32Array {
                indexBuffer = gpu.device.makeBuffer(bytes: bytes.pointer, length: bytes.length, options: .storageModeShared)
                indexType = .uint32
                indexCount = bytes.length / 4
            }
        }
        let vertexCount = position.length / 12
        if let normal = normal, normal.length < vertexCount * 12 {
            return
        }
        if let color = color, color.length < vertexCount * colorSize * 4 {
            return
        }
        let total = position.length + (normal?.length ?? 0) + (color?.length ?? 0) + (indexBuffer?.length ?? 0)
        let geometry = Geometry(positions: position, normals: normal, colors: color, colorSize: color == nil ? 0 : UInt32(colorSize), index: indexBuffer, indexType: indexType, vertexCount: vertexCount, indexCount: indexCount)
        lock.lock()
        let previous = geometries[id].map { $0.positions.length + ($0.normals?.length ?? 0) + ($0.colors?.length ?? 0) + ($0.index?.length ?? 0) } ?? 0
        if geometryBytes + instanceBytes - previous + total <= HTML3DSession.maxGeometryBytes {
            geometryBytes += total - previous
            geometries[id] = geometry
        }
        lock.unlock()
    }

    private func releaseGeometry(id: Int) {
        lock.lock()
        if let geometry = geometries.removeValue(forKey: id) {
            geometryBytes -= geometry.positions.length + (geometry.normals?.length ?? 0) + (geometry.colors?.length ?? 0) + (geometry.index?.length ?? 0)
        }
        lock.unlock()
    }

    private func defineInstances(id: Int, matrices: JSValue, count: Int, colors: JSValue) {
        guard let gpu = gpu, let source = HTML3DSession.bytes(of: matrices), source.type == kJSTypedArrayTypeFloat32Array, source.length >= 64 else {
            return
        }
        let instanceCount = max(0, min(count, source.length / 64))
        var colorSource = HTML3DSession.bytes(of: colors)
        if let bytes = colorSource, bytes.type != kJSTypedArrayTypeFloat32Array || bytes.length < instanceCount * 12 {
            colorSource = nil
        }
        lock.lock()
        defer { lock.unlock() }
        var slots = instanceSlots[id] ?? InstanceSlots()
        let slot = slots.next
        slots.next = (slot + 1) % slots.matrices.count
        func fill(_ buffer: MTLBuffer?, _ bytes: (pointer: UnsafeMutableRawPointer, length: Int, type: JSTypedArrayType)) -> MTLBuffer? {
            var target = buffer
            if target == nil || target!.length < bytes.length {
                let length = bytes.length + bytes.length / 2
                let added = length - (buffer?.length ?? 0)
                guard geometryBytes + instanceBytes + added <= HTML3DSession.maxGeometryBytes, let created = gpu.device.makeBuffer(length: length, options: .storageModeShared) else {
                    return nil
                }
                instanceBytes += added
                target = created
            }
            target?.contents().copyMemory(from: bytes.pointer, byteCount: bytes.length)
            return target
        }
        guard let matrixBuffer = fill(slots.matrices[slot], source) else {
            return
        }
        slots.matrices[slot] = matrixBuffer
        var colorBuffer: MTLBuffer?
        if let colorSource = colorSource {
            colorBuffer = fill(slots.colors[slot], colorSource)
            slots.colors[slot] = colorBuffer
        }
        instanceSlots[id] = slots
        instanceSets[id] = InstanceSet(matrices: matrixBuffer, colors: colorBuffer, count: instanceCount)
    }

    private func releaseInstances(id: Int) {
        lock.lock()
        instanceSets.removeValue(forKey: id)
        if let slots = instanceSlots.removeValue(forKey: id) {
            instanceBytes -= (slots.matrices + slots.colors).reduce(0) { $0 + ($1?.length ?? 0) }
        }
        lock.unlock()
    }

    private func submit(data: JSValue, count: Int) {
        guard isSupported, let bytes = HTML3DSession.bytes(of: data), bytes.type == kJSTypedArrayTypeFloat32Array else {
            return
        }
        let floats = min(count, bytes.length / 4, HTML3DSession.maxFloats)
        if floats < HTML3DLayout.header {
            return
        }
        lock.lock()
        if pending.count < floats {
            pending = [Float](repeating: 0, count: max(floats, pending.count * 2))
        }
        pending.withUnsafeMutableBytes { target in
            target.baseAddress!.copyMemory(from: bytes.pointer, byteCount: floats * 4)
        }
        pendingCount = floats
        hasPending = true
        statSubmitted += 1
        let schedule = !renderScheduled && layer != nil
        if schedule {
            renderScheduled = true
        }
        lock.unlock()
        if schedule {
            renderQueue.async { [self] in
                renderLoop()
            }
        }
    }

    private func renderLoop() {
        while true {
            lock.lock()
            guard hasPending, let layer = layer else {
                renderScheduled = false
                lock.unlock()
                return
            }
            swap(&pending, &working)
            let count = pendingCount
            hasPending = false
            let geometries = self.geometries
            let instanceSets = self.instanceSets
            lock.unlock()
            inFlight.wait()
            let committed = autoreleasepool {
                return encode(count: count, layer: layer, geometries: geometries, instanceSets: instanceSets)
            }
            if !committed {
                inFlight.signal()
            }
        }
    }

    private func encode(count: Int, layer: CAMetalLayer, geometries: [Int: Geometry], instanceSets: [Int: InstanceSet]) -> Bool {
        guard let gpu = gpu, let pipelines = gpu.pipelines else {
            return false
        }
        let cpuStart = clock_gettime_nsec_np(CLOCK_THREAD_CPUTIME_ID)
        return working.withUnsafeBufferPointer { buffer -> Bool in
            let f = buffer.baseAddress!
            var uniforms = HTML3DFrameUniforms()
            let remap = simd_float4x4(columns: (SIMD4<Float>(1, 0, 0, 0), SIMD4<Float>(0, 1, 0, 0), SIMD4<Float>(0, 0, 0.5, 0), SIMD4<Float>(0, 0, 0.5, 1)))
            uniforms.view = HTML3DSession.matrix(f + HTML3DLayout.view)
            uniforms.projection = remap * HTML3DSession.matrix(f + HTML3DLayout.projection)
            uniforms.shadowViewProjection = remap * HTML3DSession.matrix(f + HTML3DLayout.shadowViewProjection)
            uniforms.cameraPosition = HTML3DSession.vector(f + HTML3DLayout.camera, 1)
            uniforms.ambient = HTML3DSession.vector(f + HTML3DLayout.ambient, 0)
            uniforms.hemiSky = HTML3DSession.vector(f + HTML3DLayout.hemiSky, f[HTML3DLayout.hemiSky + 3])
            uniforms.hemiGround = HTML3DSession.vector(f + HTML3DLayout.hemiGround, 0)
            uniforms.hemiDirection = HTML3DSession.vector(f + HTML3DLayout.hemiDirection, 0)
            uniforms.directionalColor = HTML3DSession.vector(f + HTML3DLayout.directionalColor, f[HTML3DLayout.directionalColor + 3])
            uniforms.directionalDirection = HTML3DSession.vector(f + HTML3DLayout.directionalDirection, 0)
            uniforms.shadowParams = SIMD4<Float>(f[HTML3DLayout.shadow], f[HTML3DLayout.shadow + 1], f[HTML3DLayout.shadow + 2], f[HTML3DLayout.shadow + 3])
            uniforms.fogColor = SIMD4<Float>(f[HTML3DLayout.fog], f[HTML3DLayout.fog + 1], f[HTML3DLayout.fog + 2], f[HTML3DLayout.fog + 3])
            uniforms.fogParams = SIMD4<Float>(f[HTML3DLayout.fogParams], f[HTML3DLayout.fogParams + 1], f[HTML3DLayout.fogParams + 2], 0)
            let lightCount = HTML3DSession.count(f[HTML3DLayout.lightCount], upTo: HTML3DLayout.maxLights)
            var positions = [SIMD4<Float>](repeating: .init(repeating: 0), count: 4)
            var colors = positions, directions = positions, cones = positions
            for i in 0 ..< lightCount {
                let l = f + HTML3DLayout.lights + i * 16
                positions[i] = SIMD4<Float>(l[0], l[1], l[2], l[3])
                colors[i] = SIMD4<Float>(l[4], l[5], l[6], l[7])
                directions[i] = SIMD4<Float>(l[8], l[9], l[10], l[11])
                cones[i] = SIMD4<Float>(l[12], l[13], 0, 0)
            }
            uniforms.lightPosition = (positions[0], positions[1], positions[2], positions[3])
            uniforms.lightColor = (colors[0], colors[1], colors[2], colors[3])
            uniforms.lightDirection = (directions[0], directions[1], directions[2], directions[3])
            uniforms.lightCone = (cones[0], cones[1], cones[2], cones[3])
            uniforms.counts = SIMD4<Int32>(Int32(lightCount), 0, 0, 0)

            let recordCount = HTML3DSession.count(f[HTML3DLayout.recordCount], upTo: (count - HTML3DLayout.header) / HTML3DLayout.record)
            var batches: [Batch] = []
            var opaqueIndex: [BatchKey: Int] = [:]
            var instanceTotal = 0
            let view = uniforms.view
            for r in 0 ..< recordCount {
                let p = f + HTML3DLayout.header + r * HTML3DLayout.record
                let geometryId = HTML3DSession.count(p[HTML3DLayout.recordGeometry], upTo: Int(Int32.max))
                guard let geometry = geometries[geometryId] else {
                    continue
                }
                let flags = HTML3DSession.count(p[HTML3DLayout.recordFlags], upTo: 0xFFFF)
                let materialFlags = HTML3DSession.count(p[HTML3DLayout.recordParams + 2], upTo: 0xFFFF)
                let primitive = (flags >> 4) & 3
                let available = geometry.index != nil ? geometry.indexCount : geometry.vertexCount
                let start = HTML3DSession.count(p[HTML3DLayout.recordStart], upTo: available)
                let drawCount = min(HTML3DSession.count(p[HTML3DLayout.recordCount_], upTo: available), available - start)
                if drawCount <= 0 {
                    continue
                }
                let matrix = HTML3DSession.matrix(p + HTML3DLayout.recordMatrix)
                let instanceId = HTML3DSession.count(p[HTML3DLayout.recordInstances], upTo: Int(Int32.max))
                let transparent = materialFlags & 1 != 0
                let receive = flags & 2 != 0
                var shaderFlags = UInt32(materialFlags & 0x47)
                if receive {
                    shaderFlags |= 512
                }
                let vertexColors = materialFlags & 2 != 0
                var draw = HTML3DDrawUniforms()
                draw.color = SIMD4<Float>(p[HTML3DLayout.recordColor], p[HTML3DLayout.recordColor + 1], p[HTML3DLayout.recordColor + 2], p[HTML3DLayout.recordColor + 3])
                draw.emissive = SIMD4<Float>(p[HTML3DLayout.recordEmissive], p[HTML3DLayout.recordEmissive + 1], p[HTML3DLayout.recordEmissive + 2], p[HTML3DLayout.recordEmissive + 3])
                draw.specular = SIMD4<Float>(p[HTML3DLayout.recordSpecular], p[HTML3DLayout.recordSpecular + 1], p[HTML3DLayout.recordSpecular + 2], max(p[HTML3DLayout.recordSpecular + 3], 0.0001))
                draw.params = SIMD4<Float>(p[HTML3DLayout.recordParams], p[HTML3DLayout.recordParams + 1], Float(materialFlags), p[HTML3DLayout.recordParams + 3])
                if instanceId > 0 {
                    guard let set = instanceSets[instanceId] else {
                        continue
                    }
                    let instanceCount = HTML3DSession.count(p[HTML3DLayout.recordInstanceCount], upTo: set.count)
                    if instanceCount == 0 {
                        continue
                    }
                    draw.base = matrix
                    draw.layout = SIMD4<UInt32>(geometry.normals != nil ? 1 : 0, vertexColors ? geometry.colorSize : 0, set.colors != nil ? 1 : 0, shaderFlags)
                    batches.append(Batch(geometry: geometry, draw: draw, primitive: primitive, materialFlags: materialFlags, castShadow: flags & 1 != 0, start: start, count: drawCount, matrices: [], instanceSet: set, instanceCount: instanceCount, instanceOffset: 0, depth: HTML3DSession.viewDepth(view, matrix)))
                    continue
                }
                draw.layout = SIMD4<UInt32>(geometry.normals != nil ? 1 : 0, vertexColors ? geometry.colorSize : 0, 0, shaderFlags)
                instanceTotal += 1
                if transparent {
                    batches.append(Batch(geometry: geometry, draw: draw, primitive: primitive, materialFlags: materialFlags, castShadow: flags & 1 != 0, start: start, count: drawCount, matrices: [matrix], instanceSet: nil, instanceCount: 1, instanceOffset: 0, depth: HTML3DSession.viewDepth(view, matrix)))
                    continue
                }
                let key = BatchKey(geometry: geometryId, material: HTML3DSession.count(p[HTML3DLayout.recordMaterial], upTo: Int(Int32.max)), flags: flags, start: start, count: drawCount)
                if let index = opaqueIndex[key] {
                    batches[index].matrices.append(matrix)
                    batches[index].instanceCount += 1
                } else {
                    opaqueIndex[key] = batches.count
                    batches.append(Batch(geometry: geometry, draw: draw, primitive: primitive, materialFlags: materialFlags, castShadow: flags & 1 != 0, start: start, count: drawCount, matrices: [matrix], instanceSet: nil, instanceCount: 1, instanceOffset: 0, depth: 0))
                }
            }

            let ringBytes = max(instanceTotal, 1) * 64
            ringIndex = (ringIndex + 1) % ring.count
            if ring[ringIndex] == nil || ring[ringIndex]!.length < ringBytes {
                ring[ringIndex] = gpu.device.makeBuffer(length: max(ringBytes, (ring[ringIndex]?.length ?? 0) * 2, 64 * 256), options: .storageModeShared)
            }
            guard let instanceBuffer = ring[ringIndex] else {
                return false
            }
            var offset = 0
            let target = instanceBuffer.contents()
            for index in batches.indices where batches[index].instanceSet == nil {
                batches[index].instanceOffset = offset
                batches[index].matrices.withUnsafeBytes { bytes in
                    (target + offset).copyMemory(from: bytes.baseAddress!, byteCount: bytes.count)
                }
                offset += batches[index].matrices.count * 64
            }

            guard let drawable = layer.nextDrawable(), let commandBuffer = gpu.queue.makeCommandBuffer() else {
                return false
            }
            let width = drawable.texture.width
            let height = drawable.texture.height
            uniforms.viewport = SIMD4<Float>(Float(width), Float(height), max(f[HTML3DLayout.pixelRatio], 1), 0)
            guard ensureTargets(gpu: gpu, width: width, height: height) else {
                return false
            }

            let shadowsOn = uniforms.shadowParams.w > 0.5 && uniforms.directionalColor.w > 0.5 && batches.contains(where: { $0.castShadow && $0.primitive == 0 })
            var shadowTexture = gpu.dummyDepth
            if shadowsOn, let map = ensureShadowMap(gpu: gpu, size: HTML3DSession.count(f[HTML3DLayout.shadowMapSize], upTo: 4096)) {
                shadowTexture = map
                uniforms.shadowSize = SIMD4<Float>(Float(map.width), 1 / Float(map.width), 0, 0)
                let pass = MTLRenderPassDescriptor()
                pass.depthAttachment.texture = map
                pass.depthAttachment.loadAction = .clear
                pass.depthAttachment.storeAction = .store
                pass.depthAttachment.clearDepth = 1
                if let encoder = commandBuffer.makeRenderCommandEncoder(descriptor: pass) {
                    encoder.setRenderPipelineState(pipelines.shadow)
                    encoder.setDepthStencilState(pipelines.shadowDepth)
                    encoder.setFrontFacing(.counterClockwise)
                    encoder.setDepthBias(0, slopeScale: 1.5, clamp: 0.02)
                    encoder.setVertexBytes(&uniforms, length: MemoryLayout<HTML3DFrameUniforms>.stride, index: 6)
                    for batch in batches where batch.castShadow && batch.primitive == 0 {
                        var draw = batch.draw
                        let side = (batch.materialFlags >> 4) & 3
                        encoder.setCullMode(side == 0 ? .front : side == 1 ? .back : .none)
                        encoder.setVertexBuffer(batch.geometry.positions, offset: 0, index: 0)
                        if let set = batch.instanceSet {
                            encoder.setVertexBuffer(set.matrices, offset: 0, index: 3)
                        } else {
                            encoder.setVertexBuffer(instanceBuffer, offset: batch.instanceOffset, index: 3)
                        }
                        encoder.setVertexBytes(&draw, length: MemoryLayout<HTML3DDrawUniforms>.stride, index: 5)
                        HTML3DSession.draw(encoder, batch: batch, type: .triangle)
                    }
                    encoder.endEncoding()
                }
            } else {
                uniforms.shadowParams.w = 0
            }

            let pass = MTLRenderPassDescriptor()
            pass.colorAttachments[0].texture = colorTarget
            pass.colorAttachments[0].resolveTexture = drawable.texture
            pass.colorAttachments[0].loadAction = .clear
            pass.colorAttachments[0].storeAction = .multisampleResolve
            let clear = SIMD4<Float>(f[HTML3DLayout.clear], f[HTML3DLayout.clear + 1], f[HTML3DLayout.clear + 2], f[HTML3DLayout.clear + 3])
            pass.colorAttachments[0].clearColor = MTLClearColor(red: Double(clear.x * clear.w), green: Double(clear.y * clear.w), blue: Double(clear.z * clear.w), alpha: Double(clear.w))
            pass.depthAttachment.texture = depthTarget
            pass.depthAttachment.loadAction = .clear
            pass.depthAttachment.storeAction = .dontCare
            pass.depthAttachment.clearDepth = 1
            guard let encoder = commandBuffer.makeRenderCommandEncoder(descriptor: pass) else {
                return false
            }
            encoder.setFrontFacing(.counterClockwise)
            encoder.setVertexBytes(&uniforms, length: MemoryLayout<HTML3DFrameUniforms>.stride, index: 6)
            encoder.setFragmentBytes(&uniforms, length: MemoryLayout<HTML3DFrameUniforms>.stride, index: 1)
            encoder.setFragmentTexture(shadowTexture, index: 0)
            encoder.setFragmentSamplerState(gpu.shadowSampler, index: 0)
            let opaque = batches.filter { $0.materialFlags & 1 == 0 }
            let transparent = batches.filter { $0.materialFlags & 1 != 0 }.sorted { $0.depth > $1.depth }
            var triangles = 0
            for batch in opaque + transparent {
                if batch.primitive == 0 {
                    triangles += batch.count / 3 * batch.instanceCount
                }
                let blend = batch.materialFlags & 1 != 0
                let points = batch.primitive == 3
                encoder.setRenderPipelineState(points ? (blend ? pipelines.pointsBlend : pipelines.points) : (blend ? pipelines.meshBlend : pipelines.mesh))
                let depthTest = batch.materialFlags & 256 != 0
                let depthWrite = batch.materialFlags & 128 != 0
                encoder.setDepthStencilState(pipelines.depthStates[(depthTest ? 1 : 0) | (depthWrite ? 2 : 0)])
                let side = (batch.materialFlags >> 4) & 3
                encoder.setCullMode(batch.primitive != 0 ? .none : side == 0 ? .back : side == 1 ? .front : .none)
                encoder.setTriangleFillMode(batch.materialFlags & 8 != 0 ? .lines : .fill)
                encoder.setVertexBuffer(batch.geometry.positions, offset: 0, index: 0)
                encoder.setVertexBuffer(batch.geometry.normals ?? gpu.dummyBuffer, offset: 0, index: 1)
                encoder.setVertexBuffer(batch.geometry.colors ?? gpu.dummyBuffer, offset: 0, index: 2)
                if let set = batch.instanceSet {
                    encoder.setVertexBuffer(set.matrices, offset: 0, index: 3)
                    encoder.setVertexBuffer(set.colors ?? gpu.dummyBuffer, offset: 0, index: 4)
                } else {
                    encoder.setVertexBuffer(instanceBuffer, offset: batch.instanceOffset, index: 3)
                    encoder.setVertexBuffer(gpu.dummyBuffer, offset: 0, index: 4)
                }
                var draw = batch.draw
                encoder.setVertexBytes(&draw, length: MemoryLayout<HTML3DDrawUniforms>.stride, index: 5)
                encoder.setFragmentBytes(&draw, length: MemoryLayout<HTML3DDrawUniforms>.stride, index: 0)
                let type: MTLPrimitiveType = batch.primitive == 1 ? .line : batch.primitive == 2 ? .lineStrip : points ? .point : .triangle
                HTML3DSession.draw(encoder, batch: batch, type: type)
            }
            encoder.endEncoding()
            let semaphore = inFlight
            commandBuffer.addCompletedHandler { [weak self] buffer in
                var seconds: Double = 0
                if #available(macOS 10.15, *) {
                    seconds = max(0, buffer.gpuEndTime - buffer.gpuStartTime)
                }
                self?.recordCompleted(gpuSeconds: seconds)
                semaphore.signal()
            }
            commandBuffer.present(drawable)
            commandBuffer.commit()
            var memory = ring.reduce(0) { $0 + ($1?.length ?? 0) } + width * height * 4 * 3
            memory += (colorTarget?.allocatedSize ?? 0) + (depthTarget?.allocatedSize ?? 0) + (shadowMap?.allocatedSize ?? 0)
            let cpuSeconds = Double(clock_gettime_nsec_np(CLOCK_THREAD_CPUTIME_ID) - cpuStart) / 1_000_000_000
            lock.lock()
            statCPUSeconds += cpuSeconds
            statDraws = opaque.count + transparent.count
            statTriangles = triangles
            statMemory = memory + geometryBytes + instanceBytes
            lock.unlock()
            return true
        }
    }

    private func recordCompleted(gpuSeconds: Double) {
        lock.lock()
        statFrames += 1
        statGPUSeconds += gpuSeconds
        lock.unlock()
    }

    private static func draw(_ encoder: MTLRenderCommandEncoder, batch: Batch, type: MTLPrimitiveType) {
        if let index = batch.geometry.index {
            let size = batch.geometry.indexType == .uint16 ? 2 : 4
            encoder.drawIndexedPrimitives(type: type, indexCount: batch.count, indexType: batch.geometry.indexType, indexBuffer: index, indexBufferOffset: batch.start * size, instanceCount: batch.instanceCount)
        } else {
            encoder.drawPrimitives(type: type, vertexStart: batch.start, vertexCount: batch.count, instanceCount: batch.instanceCount)
        }
    }

    private func ensureTargets(gpu: HTML3DGPU, width: Int, height: Int) -> Bool {
        if let color = colorTarget, color.width == width, color.height == height, depthTarget != nil {
            return true
        }
        let colorDesc = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: HTML3DGPU.colorFormat, width: width, height: height, mipmapped: false)
        colorDesc.textureType = .type2DMultisample
        colorDesc.sampleCount = HTML3DGPU.sampleCount
        colorDesc.usage = .renderTarget
        let depthDesc = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: HTML3DGPU.depthFormat, width: width, height: height, mipmapped: false)
        depthDesc.textureType = .type2DMultisample
        depthDesc.sampleCount = HTML3DGPU.sampleCount
        depthDesc.usage = .renderTarget
        if gpu.memorylessTargets, #available(macOS 11.0, *) {
            colorDesc.storageMode = .memoryless
            depthDesc.storageMode = .memoryless
        } else {
            colorDesc.storageMode = .private
            depthDesc.storageMode = .private
        }
        colorTarget = gpu.device.makeTexture(descriptor: colorDesc)
        depthTarget = gpu.device.makeTexture(descriptor: depthDesc)
        return colorTarget != nil && depthTarget != nil
    }

    private func ensureShadowMap(gpu: HTML3DGPU, size requested: Int) -> MTLTexture? {
        let size = max(256, min(requested > 0 ? requested : 1024, 4096))
        if let map = shadowMap, map.width == size {
            return map
        }
        let desc = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: HTML3DGPU.depthFormat, width: size, height: size, mipmapped: false)
        desc.usage = [.renderTarget, .shaderRead]
        desc.storageMode = .private
        shadowMap = gpu.device.makeTexture(descriptor: desc)
        return shadowMap
    }

    private static func matrix(_ p: UnsafePointer<Float>) -> simd_float4x4 {
        return simd_float4x4(columns: (SIMD4<Float>(p[0], p[1], p[2], p[3]), SIMD4<Float>(p[4], p[5], p[6], p[7]), SIMD4<Float>(p[8], p[9], p[10], p[11]), SIMD4<Float>(p[12], p[13], p[14], p[15])))
    }

    private static func vector(_ p: UnsafePointer<Float>, _ w: Float) -> SIMD4<Float> {
        return SIMD4<Float>(p[0], p[1], p[2], w)
    }

    private static func viewDepth(_ view: simd_float4x4, _ model: simd_float4x4) -> Float {
        let position = view * model.columns.3
        return -position.z
    }
}
