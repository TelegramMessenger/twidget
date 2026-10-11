import AppKit

public final class MagicMoveStage {
    public struct Snapshot {
        public let image: CGImage
        public let ink: CGRect
        public let border: CGRect

        public init(image: CGImage, ink: CGRect, border: CGRect) {
            self.image = image
            self.ink = ink
            self.border = border
        }
    }

    public enum Item {
        case move(key: String, from: CGRect, to: Snapshot, previous: Snapshot?)
        case enter(key: String, Snapshot)
        case leave(Snapshot)
    }

    private struct Active {
        let layer: CALayer
        let target: CGRect
    }

    public static let duration: CFTimeInterval = 0.55
    public static let maxLayers = 48
    public static let maxPixels = 4096 * 4096

    private weak var host: CALayer?
    private var active: [String: Active] = [:]
    private var extra: [CALayer] = []
    private var generation = 0
    public var onFinish: (() -> Void)?

    public init(host: CALayer?) {
        self.host = host
    }

    public var isActive: Bool {
        return !active.isEmpty || !extra.isEmpty
    }

    public func presentedFrames() -> [String: CGRect] {
        var frames: [String: CGRect] = [:]
        for (key, item) in active {
            let layer = item.layer.presentation() ?? item.layer
            let t = CATransform3DGetAffineTransform(layer.transform)
            frames[key] = CGRect(x: layer.position.x + t.tx, y: layer.position.y + t.ty, width: item.target.width * t.a, height: item.target.height * t.d)
        }
        return frames
    }

    public func play(_ items: [Item], scale: CGFloat) {
        guard let host = host else {
            return
        }
        generation += 1
        let current = generation
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        clear()
        for item in items {
            switch item {
            case let .move(key, from, to, previous):
                guard let layer = MagicMoveStage.layer(to, scale: scale) else {
                    continue
                }
                layer.zPosition = 2
                host.addSublayer(layer)
                layer.add(MagicMoveStage.spring(from: MagicMoveStage.mapping(from: to.border, to: from), to: CATransform3DIdentity), forKey: "move")
                if let previous = previous, let outgoing = MagicMoveStage.layer(previous, scale: scale) {
                    outgoing.zPosition = 2
                    outgoing.opacity = 0
                    outgoing.transform = MagicMoveStage.mapping(from: previous.border, to: to.border)
                    host.insertSublayer(outgoing, below: layer)
                    outgoing.add(MagicMoveStage.spring(from: MagicMoveStage.mapping(from: previous.border, to: from), to: outgoing.transform), forKey: "move")
                    let fadeOut = MagicMoveStage.fade(from: 1, to: 0, duration: 0.16)
                    fadeOut.beginTime = CACurrentMediaTime() + 0.2
                    outgoing.add(fadeOut, forKey: "fade")
                    layer.add(MagicMoveStage.fade(from: 0, to: 1, duration: 0.24), forKey: "fade")
                    extra.append(outgoing)
                }
                active[key] = Active(layer: layer, target: to.border)
            case let .enter(key, snapshot):
                guard let layer = MagicMoveStage.layer(snapshot, scale: scale) else {
                    continue
                }
                layer.zPosition = 2
                host.addSublayer(layer)
                layer.add(MagicMoveStage.spring(from: MagicMoveStage.scaled(layer, 0.92), to: CATransform3DIdentity), forKey: "enter")
                layer.add(MagicMoveStage.fade(from: 0, to: 1, duration: 0.22, timing: CAMediaTimingFunction(controlPoints: 0.33, 1, 0.68, 1)), forKey: "fade")
                active[key] = Active(layer: layer, target: snapshot.border)
            case let .leave(snapshot):
                guard let layer = MagicMoveStage.layer(snapshot, scale: scale) else {
                    continue
                }
                layer.zPosition = 1
                layer.opacity = 0
                layer.transform = MagicMoveStage.scaled(layer, 0.92)
                host.addSublayer(layer)
                layer.add(MagicMoveStage.fade(from: 1, to: 0, duration: 0.18, timing: CAMediaTimingFunction(controlPoints: 0.32, 0, 0.67, 0)), forKey: "fade")
                layer.add(MagicMoveStage.spring(from: CATransform3DIdentity, to: layer.transform), forKey: "leave")
                extra.append(layer)
            }
        }
        CATransaction.commit()
        DispatchQueue.main.asyncAfter(deadline: .now() + MagicMoveStage.duration) { [weak self] in
            guard let self = self, self.generation == current else {
                return
            }
            self.onFinish?()
        }
    }

    public func clear() {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        for item in active.values {
            item.layer.removeFromSuperlayer()
        }
        for layer in extra {
            layer.removeFromSuperlayer()
        }
        CATransaction.commit()
        active.removeAll()
        extra.removeAll()
    }

    public func cancel() {
        generation += 1
        clear()
    }

    public func draw(into ctx: CGContext) {
        for layer in (extra + active.values.map { $0.layer }).sorted(by: { $0.zPosition < $1.zPosition }) {
            let model = layer.presentation() ?? layer
            guard let contents = layer.contents, CFGetTypeID(contents as CFTypeRef) == CGImage.typeID else {
                continue
            }
            let size = model.bounds.size
            ctx.saveGState()
            ctx.translateBy(x: model.position.x, y: model.position.y)
            ctx.concatenate(CATransform3DGetAffineTransform(model.transform))
            ctx.translateBy(x: -model.anchorPoint.x * size.width, y: -model.anchorPoint.y * size.height)
            ctx.setAlpha(CGFloat(model.opacity))
            ctx.translateBy(x: 0, y: size.height)
            ctx.scaleBy(x: 1, y: -1)
            ctx.draw(contents as! CGImage, in: CGRect(origin: .zero, size: size))
            ctx.restoreGState()
        }
    }

    public static func close(_ a: CGRect, _ b: CGRect) -> Bool {
        return abs(a.minX - b.minX) < 0.5 && abs(a.minY - b.minY) < 0.5 && abs(a.width - b.width) < 0.5 && abs(a.height - b.height) < 0.5
    }

    private static func layer(_ snapshot: Snapshot, scale: CGFloat) -> CALayer? {
        let ink = snapshot.ink, border = snapshot.border
        guard ink.width > 0, ink.height > 0 else {
            return nil
        }
        let layer = CALayer()
        layer.contents = snapshot.image
        layer.contentsScale = scale
        layer.contentsGravity = .resize
        layer.bounds = CGRect(origin: .zero, size: ink.size)
        layer.anchorPoint = CGPoint(x: (border.minX - ink.minX) / ink.width, y: (border.minY - ink.minY) / ink.height)
        layer.position = border.origin
        return layer
    }

    private static func mapping(from a: CGRect, to b: CGRect) -> CATransform3D {
        let sx = a.width > 0 ? b.width / a.width : 1
        let sy = a.height > 0 ? b.height / a.height : 1
        return CATransform3DConcat(CATransform3DMakeScale(sx, sy, 1), CATransform3DMakeTranslation(b.minX - a.minX, b.minY - a.minY, 0))
    }

    private static func scaled(_ layer: CALayer, _ factor: CGFloat) -> CATransform3D {
        let size = layer.bounds.size
        let center = CGPoint(x: size.width / 2 - layer.anchorPoint.x * size.width, y: size.height / 2 - layer.anchorPoint.y * size.height)
        var transform = CATransform3DMakeTranslation(-center.x, -center.y, 0)
        transform = CATransform3DConcat(transform, CATransform3DMakeScale(factor, factor, 1))
        return CATransform3DConcat(transform, CATransform3DMakeTranslation(center.x, center.y, 0))
    }

    private static func spring(from: CATransform3D, to: CATransform3D) -> CAAnimation {
        let spring = CASpringAnimation(keyPath: "transform")
        spring.fromValue = NSValue(caTransform3D: from)
        spring.toValue = NSValue(caTransform3D: to)
        spring.mass = 1
        spring.stiffness = 320
        spring.damping = 30
        spring.duration = duration
        spring.fillMode = .both
        return spring
    }

    private static func fade(from: Float, to: Float, duration: CFTimeInterval, timing: CAMediaTimingFunction = CAMediaTimingFunction(controlPoints: 0.45, 0, 0.55, 1)) -> CABasicAnimation {
        let fade = CABasicAnimation(keyPath: "opacity")
        fade.fromValue = from
        fade.toValue = to
        fade.duration = duration
        fade.timingFunction = timing
        fade.fillMode = .both
        return fade
    }
}
