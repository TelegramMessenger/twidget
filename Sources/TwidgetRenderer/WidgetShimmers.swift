import Foundation
import QuartzCore
import LiteHTML

final class WidgetShimmers {
    static let enabled = ProcessInfo.processInfo.environment["TW_NO_SHIMMER"] == nil
    static let properties: Set<String> = ["background-position", "background-position-x"]

    private struct Item {
        let container: CALayer
        let tile: CALayer
        let tileRect: CGRect
        let area: CGRect
        let clip: CGRect
        var x: CGFloat
    }

    private var items: [Int: Item] = [:]
    private var refused: Set<Int> = []
    private weak var host: CALayer?
    private(set) var updates = 0

    var ids: Set<Int> {
        return Set(items.keys)
    }

    func reset() {
        for item in items.values {
            item.container.removeFromSuperlayer()
        }
        items.removeAll()
    }

    func update(document: LHDocument, animator: WidgetAnimator, width: CGFloat, host: CALayer?, scale: CGFloat, composited: Set<Int>, suspended: Bool) -> Bool {
        guard WidgetShimmers.enabled else {
            return false
        }
        var wanted: Set<Int> = []
        if !suspended {
            for id in animator.animatedIDs() where !refused.contains(id) && !composited.contains(id) && animator.animatesOnly(id, WidgetShimmers.properties) {
                wanted.insert(id)
            }
        }
        var changed = false
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        if self.host !== host {
            self.host = host
            for item in items.values {
                host?.addSublayer(item.container)
            }
        }
        for id in Set(items.keys).subtracting(wanted) {
            items.removeValue(forKey: id)?.container.removeFromSuperlayer()
            changed = true
        }
        for id in wanted where items[id] == nil {
            guard let tile = document.backgroundTile(attribute: WidgetDOM.idAttribute, value: String(id), maxWidth: width, scale: scale) else {
                refused.insert(id)
                continue
            }
            let radii = (0..<8).map { tile.radius($0) }
            guard radii.allSatisfy({ abs($0 - radii[0]) < 0.01 }) else {
                refused.insert(id)
                continue
            }
            let container = CALayer()
            container.frame = tile.clip
            container.masksToBounds = true
            container.cornerRadius = radii[0]
            let layer = CALayer()
            layer.contents = tile.image
            layer.contentsScale = scale
            layer.frame = tile.tile.offsetBy(dx: -tile.clip.minX, dy: -tile.clip.minY)
            container.addSublayer(layer)
            host?.addSublayer(container)
            items[id] = Item(container: container, tile: layer, tileRect: tile.tile, area: tile.area, clip: tile.clip, x: tile.tile.minX)
            changed = true
        }
        CATransaction.commit()
        return changed
    }

    func apply(_ overrides: inout [Int: [(String, String)]]) {
        guard !items.isEmpty else {
            return
        }
        var moved = false
        var failed: Set<Int> = []
        for (id, item) in items {
            guard let list = overrides[id] else {
                continue
            }
            var rest: [(String, String)] = []
            for (name, value) in list {
                if WidgetShimmers.properties.contains(name) {
                    guard let x = position(value, item: item) else {
                        failed.insert(id)
                        rest.append((name, value))
                        continue
                    }
                    if x != item.x {
                        if !moved {
                            CATransaction.begin()
                            CATransaction.setDisableActions(true)
                            moved = true
                        }
                        item.tile.frame.origin.x = x - item.clip.minX
                        items[id]?.x = x
                        updates += 1
                    }
                } else {
                    rest.append((name, value))
                }
            }
            overrides[id] = rest.isEmpty ? nil : rest
        }
        if moved {
            CATransaction.commit()
        }
        if !failed.isEmpty {
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            for id in failed {
                items.removeValue(forKey: id)?.container.removeFromSuperlayer()
                refused.insert(id)
            }
            CATransaction.commit()
            dropped = true
        }
    }

    var dropped = false

    private func position(_ value: String, item: Item) -> CGFloat? {
        let scalars = Array(value.unicodeScalars)
        var index = 0
        while index < scalars.count, scalars[index] == " " {
            index += 1
        }
        let start = index
        if index < scalars.count, scalars[index] == "-" || scalars[index] == "+" {
            index += 1
        }
        while index < scalars.count, ("0"..."9").contains(scalars[index]) || scalars[index] == "." {
            index += 1
        }
        guard index > start, let number = Double(String(String.UnicodeScalarView(scalars[start..<index]))) else {
            return nil
        }
        var unitEnd = index
        while unitEnd < scalars.count, ("a"..."z").contains(scalars[unitEnd]) || scalars[unitEnd] == "%" {
            unitEnd += 1
        }
        let unit = String(String.UnicodeScalarView(scalars[index..<unitEnd]))
        switch unit {
        case "%":
            return item.area.minX + (item.area.width - item.tileRect.width) * CGFloat(number) / 100
        case "px":
            return item.area.minX + CGFloat(number)
        case "" where number == 0:
            return item.area.minX
        default:
            return nil
        }
    }
}
