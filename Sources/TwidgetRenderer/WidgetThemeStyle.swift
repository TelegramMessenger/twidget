import Foundation

enum WidgetThemeStyle {
    static let keys = [
        "bg_color", "text_color", "hint_color", "link_color", "button_color", "button_text_color",
        "secondary_bg_color", "accent_text_color", "subtitle_text_color", "destructive_text_color",
        "section_bg_color", "section_header_text_color", "section_separator_color", "header_bg_color", "bottom_bar_bg_color",
    ]

    static let defaults: [String: [String: String]] = [
        "light": [
            "bg_color": "#ffffff", "text_color": "#000000", "hint_color": "#707579", "link_color": "#3390ec",
            "button_color": "#3390ec", "button_text_color": "#ffffff", "secondary_bg_color": "#f4f4f5",
            "accent_text_color": "#3390ec", "subtitle_text_color": "#707579", "destructive_text_color": "#e53935",
            "section_bg_color": "#ffffff", "section_header_text_color": "#3390ec", "section_separator_color": "#e7e7e7",
            "header_bg_color": "#ffffff", "bottom_bar_bg_color": "#ffffff",
        ],
        "dark": [
            "bg_color": "#212121", "text_color": "#ffffff", "hint_color": "#aaaaaa", "link_color": "#8774e1",
            "button_color": "#8774e1", "button_text_color": "#ffffff", "secondary_bg_color": "#181818",
            "accent_text_color": "#8774e1", "subtitle_text_color": "#aaaaaa", "destructive_text_color": "#ff595a",
            "section_bg_color": "#212121", "section_header_text_color": "#8774e1", "section_separator_color": "#3d3d3d",
            "header_bg_color": "#212121", "bottom_bar_bg_color": "#212121",
        ],
    ]

    static let profileUAStylesheet = "@layer tw-ua{:where(button){appearance:none;-webkit-appearance:none;margin:0;border:0;border-radius:var(--tg-button-radius,8px);padding:var(--tg-button-padding,8px 14px);min-height:var(--tg-button-height,auto);font:var(--tg-button-font,500 14px/1.2 system-ui,sans-serif);background:var(--tg-button-bg,rgba(51,144,236,.12));color:var(--tg-button-text,inherit);cursor:pointer;-webkit-tap-highlight-color:transparent}:where(button:not([disabled])):active{opacity:var(--tg-button-pressed-opacity,.7)}:where(button[disabled]){opacity:var(--tg-button-disabled-opacity,.5);cursor:default}}*{-webkit-user-select:none!important;user-select:none!important;-webkit-touch-callout:none!important}"

    static let uaStylesheet = unwrappingWhere(profileUAStylesheet)

    private static let color = regex("#[0-9a-fA-F]{6}")
    private static let length = regex("(0|[0-9]{1,4}(\\.[0-9]{1,3})?(px|em|rem))")
    private static let font = regex("(?:(normal|italic) )?(?:(normal|bold|[1-9]00) )?[0-9]{1,3}(?:\\.[0-9]{1,2})?(?:px|em|rem)(?:/[0-9]{1,3}(?:\\.[0-9]{1,2})?(?:px|em|rem)?)? ([a-z-]+(?:, ?[a-z-]+)*)")
    private static let opacityValue = regex("(0(\\.[0-9]{1,3})?|1(\\.0{1,3})?|\\.[0-9]{1,3})")
    private static let padding = regex("\\S+( \\S+){0,3}")
    private static let generic: Set<String> = ["system-ui", "sans-serif", "serif", "monospace", "ui-sans-serif", "ui-serif", "ui-monospace", "ui-rounded"]

    private static let buttonVariables: [(field: String, name: String, check: (Any?) -> String?)] = [
        ("bg", "--tg-button-bg", { hexColor($0) }),
        ("text", "--tg-button-text", { hexColor($0) }),
        ("radius", "--tg-button-radius", { ($0 as? String).flatMap { matches(length, $0) ? $0 : nil } }),
        ("font", "--tg-button-font", { fontToken($0) }),
        ("padding", "--tg-button-padding", { ($0 as? String).flatMap { matches(padding, $0) && $0.components(separatedBy: " ").allSatisfy { matches(length, $0) } ? $0 : nil } }),
        ("height", "--tg-button-height", { ($0 as? String).flatMap { matches(length, $0) ? $0 : nil } }),
        ("pressedOpacity", "--tg-button-pressed-opacity", { opacity($0) }),
        ("disabledOpacity", "--tg-button-disabled-opacity", { opacity($0) }),
    ]

    static let variableNames: Set<String> = Set(keys.map(variableName) + ["--tg-color-scheme"] + buttonVariables.map { $0.name })

    static func checkParams(_ params: Any?) -> [String: String] {
        guard let params = params as? [String: Any] else {
            return [:]
        }
        var out: [String: String] = [:]
        for key in keys {
            if let value = params[key] as? String, matches(color, value) {
                out[key] = value.lowercased()
            }
        }
        return out
    }

    static func variables(_ theme: Any?) -> [(name: String, value: String)] {
        let theme = (theme as? [String: Any]) ?? [:]
        let scheme = (theme["colorScheme"] as? String) == "dark" ? "dark" : "light"
        let params = checkParams(theme["params"])
        let fallback = defaults[scheme] ?? [:]
        var out = keys.map { key -> (name: String, value: String) in (variableName(key), params[key] ?? fallback[key] ?? "") }
        out.append(("--tg-color-scheme", scheme))
        let button = (theme["button"] as? [String: Any]) ?? [:]
        for variable in buttonVariables {
            if let value = variable.check(button[variable.field]) {
                out.append((variable.name, value))
            }
        }
        return out
    }

    private static func variableName(_ key: String) -> String {
        return "--tg-theme-" + key.replacingOccurrences(of: "_", with: "-")
    }

    private static func hexColor(_ value: Any?) -> String? {
        guard let value = value as? String, matches(color, value) else {
            return nil
        }
        return value.lowercased()
    }

    private static func fontToken(_ value: Any?) -> String? {
        guard let value = value as? String, let match = font.firstMatch(in: value, range: NSRange(value.startIndex..., in: value)), let range = Range(match.range(at: 3), in: value) else {
            return nil
        }
        return value[range].split(separator: ",", omittingEmptySubsequences: false).allSatisfy { generic.contains($0.trimmingCharacters(in: .whitespaces)) } ? value : nil
    }

    private static func opacity(_ value: Any?) -> String? {
        let text: String
        if let string = value as? String {
            text = string
        } else if let number = value as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID() {
            let double = number.doubleValue
            text = double == double.rounded() && abs(double) < 1e15 ? String(Int64(double)) : "\(double)"
        } else {
            return nil
        }
        return matches(opacityValue, text) ? text : nil
    }

    private static func regex(_ pattern: String) -> NSRegularExpression {
        return try! NSRegularExpression(pattern: "^(?:" + pattern + ")\\z")
    }

    private static func matches(_ regex: NSRegularExpression, _ value: String) -> Bool {
        return regex.firstMatch(in: value, range: NSRange(value.startIndex..., in: value)) != nil
    }

    private static func unwrappingWhere(_ sheet: String) -> String {
        var out = ""
        var rest = Substring(sheet)
        while let start = rest.range(of: ":where(") {
            out += rest[..<start.lowerBound]
            var depth = 1
            var index = start.upperBound
            while index < rest.endIndex {
                if rest[index] == "(" {
                    depth += 1
                } else if rest[index] == ")" {
                    depth -= 1
                    if depth == 0 {
                        break
                    }
                }
                index = rest.index(after: index)
            }
            out += rest[start.upperBound..<index]
            rest = index < rest.endIndex ? rest[rest.index(after: index)...] : ""
        }
        return out + rest
    }
}
