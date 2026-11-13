import CoreGraphics

extension CGRect {
    var hasFiniteGeometry: Bool {
        return origin.x.isFinite && origin.y.isFinite
            && size.width >= 0 && size.height >= 0
            && (origin.x + size.width).isFinite && (origin.y + size.height).isFinite
    }
}

func widgetRasterPixelSize(width: CGFloat, height: CGFloat, scale: CGFloat) -> CGSize? {
    guard scale.isFinite, scale > 0, height.isFinite else {
        return nil
    }
    let pixelWidth = ceil(width * scale)
    let pixelHeight = ceil(max(1, ceil(height)) * scale)
    guard pixelWidth > 0, pixelWidth <= 16384, pixelHeight > 0, pixelHeight <= 16384 else {
        return nil
    }
    return CGSize(width: pixelWidth, height: pixelHeight)
}
