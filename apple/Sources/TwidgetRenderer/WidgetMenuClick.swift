import AppKit

extension LiteHTMLWidgetRenderer {
    public func menuClick(atWindowPoint location: CGPoint, event: NSEvent) -> Bool {
        return handleMenuClick(documentView.convert(location, from: nil), event: event)
    }
}
