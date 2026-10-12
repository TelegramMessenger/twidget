import Foundation

public struct WidgetOp {
    public enum Kind: UInt8 {
        case style
        case attribute
        case canvas
        case other
    }

    public let kind: Kind
    public let id: Int
    public let name: String
    public let value: String
    public let important: Bool
    public let fields: [String: Any]?
    public let canvas: WidgetCanvasBatch?

    public init(style id: Int, name: String, value: String, important: Bool) {
        kind = .style
        self.id = id
        self.name = name
        self.value = value
        self.important = important
        fields = nil
        canvas = nil
    }

    public init(canvas id: Int, batch: WidgetCanvasBatch) {
        kind = .canvas
        self.id = id
        name = ""
        value = ""
        important = false
        fields = nil
        canvas = batch
    }

    public init(attribute id: Int, name: String, value: String) {
        kind = .attribute
        self.id = id
        self.name = name
        self.value = value
        important = false
        fields = nil
        canvas = nil
    }

    public init(_ fields: [String: Any]) {
        let op = fields["op"] as? String
        if op == "style", let id = fields["id"] as? NSNumber, let name = fields["name"] as? String, fields.count <= 5 {
            let value = fields["value"]
            let important = fields["important"]
            if value == nil || value is String, important == nil || important is Bool {
                self.init(style: id.intValue, name: name, value: (value as? String) ?? "", important: (important as? Bool) ?? false)
                return
            }
        }
        self.init(kind: .other, id: (fields["id"] as? NSNumber)?.intValue ?? 0, fields: fields)
    }

    private init(kind: Kind, id: Int, fields: [String: Any]) {
        self.kind = kind
        self.id = id
        name = ""
        value = ""
        important = false
        self.fields = fields
        canvas = nil
    }

    public var op: String? {
        switch kind {
        case .style:
            return "style"
        case .attribute:
            return "attr"
        case .canvas:
            return "canvas"
        case .other:
            return fields?["op"] as? String
        }
    }

    public subscript(key: String) -> Any? {
        if kind == .other {
            return fields?[key]
        }
        switch key {
        case "op":
            return op
        case "id":
            return NSNumber(value: id)
        case "name":
            return name
        case "value":
            return value
        case "important":
            return kind == .style ? important : nil
        case "cmds":
            return canvas?.commands()
        default:
            return nil
        }
    }

    public static func json(_ ops: [WidgetOp]) -> String {
        var out = "["
        out.reserveCapacity(ops.count * 64 + 2)
        for (index, op) in ops.enumerated() {
            if index > 0 {
                out += ","
            }
            switch op.kind {
            case .style:
                out += "{\"op\":\"style\",\"id\":\(op.id),\"name\":"
                appendString(op.name, to: &out)
                out += ",\"value\":"
                appendString(op.value, to: &out)
                out += op.important ? ",\"important\":true}" : ",\"important\":false}"
            case .attribute:
                out += "{\"op\":\"attr\",\"id\":\(op.id),\"name\":"
                appendString(op.name, to: &out)
                out += ",\"value\":"
                appendString(op.value, to: &out)
                out += "}"
            case .canvas:
                out += "{\"op\":\"canvas\",\"id\":\(op.id),\"cmds\":"
                out += op.canvas?.json(number: { $0.isFinite ? "\($0)" : "null" }) ?? "[]"
                out += "}"
            case .other:
                if let fields = op.fields, JSONSerialization.isValidJSONObject(fields), let data = try? JSONSerialization.data(withJSONObject: fields) {
                    out += String(decoding: data, as: UTF8.self)
                } else {
                    out += "null"
                }
            }
        }
        return out + "]"
    }

    static func appendString(_ s: String, to out: inout String) {
        out += "\""
        for scalar in s.unicodeScalars {
            switch scalar {
            case "\"":
                out += "\\\""
            case "\\":
                out += "\\\\"
            case "\n":
                out += "\\n"
            case "\r":
                out += "\\r"
            case "\t":
                out += "\\t"
            default:
                if scalar.value < 0x20 || scalar.value == 0x2028 || scalar.value == 0x2029 {
                    out += String(format: "\\u%04x", scalar.value)
                } else {
                    out.unicodeScalars.append(scalar)
                }
            }
        }
        out += "\""
    }
}
