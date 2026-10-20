package org.telegram.widgetsandbox.litehtml;

/**
 * JNI surface of TwidgetCore's platform/android/jni.cpp: one renderer per widget behind a handle. Every call
 * on the main thread, which is where the backend's callbacks come back too.
 */
public final class LitehtmlNative {

    static {
        System.loadLibrary("twidget_litehtml");
    }

    private LitehtmlNative() {
    }

    /** `backend` answers the font, text-metric, image and canvas callbacks for the lifetime of the handle. */
    public static native long create(double widthDp, double dpr, String theme, LitehtmlBackend backend);

    public static native void destroy(long handle);

    /** Host → renderer JSON message; returns a JSON array of renderer → host messages. */
    public static native String receive(long handle, String json);

    public static native String setWidth(long handle, double widthDp);

    public static native String setMaxHeight(long handle, double heightDp);

    public static native String setTheme(long handle, String theme);

    /** action: 0 down, 1 move, 2 up, 3 cancel; coordinates in CSS px. Returns renderer → host messages. */
    public static native String touch(long handle, int action, double x, double y, int pointer, boolean primary);

    /** As touch, from a mouse (no touch events) with modifiers: bit0 alt, bit1 ctrl, bit2 meta, bit3 shift. */
    public static native String pointer(long handle, int action, double x, double y, int pointer, boolean primary, boolean mouse, int modifiers);

    /** A mouse over the view with no button down (action 0), or leaving it (1). */
    public static native String hover(long handle, int action, double x, double y, int modifiers);

    /** mode: 0 pixels, 1 lines, 2 pages. */
    public static native String wheel(long handle, double x, double y, double dx, double dy, int mode, int modifiers);

    /** type "keydown" or "keyup", with the DOM key and code. */
    public static native String key(long handle, String type, String key, String code, boolean repeat, int modifiers);

    /** The text control being edited as JSON (Renderer::EditorSpec), or "null": cover it with an EditText. */
    public static native String editor(long handle);

    /** The EditText's text changed, for editor generation `generation`. */
    public static native String edit(long handle, long generation, String text, String inputType);

    /** The EditText let go of the keyboard. */
    public static native String endEditing(long handle);

    /** One display frame (Choreographer): animations advance. Every frame while wantsTick, else after nextTickIn seconds. */
    public static native String tick(long handle);

    public static native boolean wantsTick(long handle);

    public static native double nextTickIn(long handle);

    public static native void setCompositing(long handle, boolean enabled);

    /** The layers above the last displayList (src/compositor.h, to_json(Composition)). */
    public static native String composition(long handle);

    /** The renderer clock in seconds: what the layers' track starts are on. */
    public static native double clock();

    /** Layer `index` of the last composition as a display list; null past the end. */
    public static native String layerContent(long handle, int index);

    /** (width << 32) | height, CSS px (= dp). */
    public static native long size(long handle);

    public static native boolean dirty(long handle);

    /** Records the document into a display list (JSON, src/display_list.h), CSS px; clears `dirty`. */
    public static native String displayList(long handle);

    /** Where the last displayList differs from the one before: `[[x, y, w, h], ...]`, or "null" for all. */
    public static native String damage(long handle);

    /** Drops caches that are made again on demand: from onTrimMemory, or for a widget scrolled away. */
    public static native void trim(long handle);

    /** The last rebuild as JSON: serializeMs, parseLayoutMs, passes, htmlBytes, builds, displayOps. */
    public static native String stats(long handle);

    /** Runs a display list FilterPush's "fl" over an ARGB_8888 bitmap's pixels; `scale` is its px per CSS px. */
    public static native void filterPixels(android.graphics.Bitmap bitmap, String filters, float scale);
}
