// DirectWrite fonts, WIC images and a Direct2D painter for the DisplayList: the Windows twin of
// CGBackend (platform/apple/coregraphics.cpp). One per renderer, every call under that renderer's
// lock. The host owns the devices and factories, and the device context it paints into.
#pragma once

#include <d2d1_1.h>
#include <dwrite.h>
#include <wincodec.h>
#include <winrt/base.h>

#include "backend.h"
#include "display_list.h"

#include <map>
#include <tuple>

namespace twlite
{
    class D2DBackend : public Backend
    {
    public:
        D2DBackend(ID2D1Device* device, IDWriteFactory* dwrite, IWICImagingFactory* wic);

        int create_font(const std::string& families, float size_px, int weight, bool italic, FontMetrics& out) override;
        void delete_font(int font) override;
        float text_width(int font, const std::string& text) override;
        bool load_image(int id, const std::string& url, float& w, float& h) override;
        void canvas_run(int id, int w, int h, const std::vector<CanvasOp>& ops) override;
        bool canvas_read(int id, int x, int y, int w, int h, std::vector<uint8_t>& rgba) override;
        void canvas_release(int id) override;

        // Paints `list` (CSS px) into `ctx`, whose transform already maps CSS px onto the target.
        void paint(const DisplayList& list, ID2D1DeviceContext* ctx);

    private:
        struct Layout
        {
            winrt::com_ptr<IDWriteTextLayout> layout;
            float width = 0;
            float baseline = 0;
        };

        // litehtml parses a fresh document for every batch and asks for its fonts again, so faces
        // and their measured runs are kept by description rather than by the handle it gets back.
        struct Face
        {
            winrt::com_ptr<IDWriteTextFormat> format;
            FontMetrics metrics;
            std::unordered_map<std::string, Layout> layouts;
        };

        using FaceKey = std::tuple<std::string, float, int, bool>;

        const Layout* layout(Face& face, const std::string& text);
        std::wstring resolve_family(const std::string& families);
        winrt::com_ptr<ID2D1Geometry> rounded(const Rect& r, const Radii& q);
        winrt::com_ptr<ID2D1GradientStopCollection> stops(const std::vector<Stop>& stops);
        void conic(const Command& c);
        void path(const Path& p);
        void shadow(const Command& c);
        winrt::com_ptr<ID2D1Geometry> exclude(ID2D1Geometry* outer, ID2D1Geometry* inner);
        winrt::com_ptr<ID2D1PathGeometry> geometry(const std::vector<Path::Op>& ops, const std::vector<float>& points, bool evenodd);
        winrt::com_ptr<ID2D1Brush> canvas_brush(ID2D1DeviceContext* ctx, const CanvasPaint& paint, float alpha);
        void canvas_content(ID2D1DeviceContext* ctx, const CanvasOp& op, ID2D1Bitmap* self);
        void canvas_op(const CanvasOp& op, int id);

        ID2D1Device* m_device;
        IDWriteFactory* m_dwrite;
        IWICImagingFactory* m_wic;
        winrt::com_ptr<IDWriteFontCollection> m_fonts;
        winrt::com_ptr<ID2D1Factory> m_factory;

        std::map<FaceKey, std::shared_ptr<Face>> m_faces;
        std::unordered_map<int, std::shared_ptr<Face>> m_handles;
        int m_nextFont = 0;

        struct Image
        {
            winrt::com_ptr<IWICBitmapSource> source;
            winrt::com_ptr<ID2D1Bitmap> bitmap;
        };
        std::unordered_map<int, Image> m_images;

        // Valid during paint() only.
        ID2D1DeviceContext* m_ctx = nullptr;
        winrt::com_ptr<ID2D1SolidColorBrush> m_brush;
        winrt::com_ptr<ID2D1Effect> m_blur;
        // Records shadow shapes into command lists while m_ctx is drawing.
        winrt::com_ptr<ID2D1DeviceContext> m_recorder;

        // Canvas bitmaps, drawn by their own context: canvas commands arrive between paints.
        struct CanvasBitmap
        {
            winrt::com_ptr<ID2D1Bitmap1> bitmap;
            int w = 0;
            int h = 0;
        };
        std::unordered_map<int, CanvasBitmap> m_canvases;
        winrt::com_ptr<ID2D1DeviceContext> m_canvasCtx;
        winrt::com_ptr<ID2D1DeviceContext> m_canvasRecorder;
        winrt::com_ptr<ID2D1SolidColorBrush> m_canvasBrush;
    };
}
