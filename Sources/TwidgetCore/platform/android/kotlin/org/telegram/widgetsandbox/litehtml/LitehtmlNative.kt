package org.telegram.widgetsandbox.litehtml

/** JNI surface of platform/android/jni.cpp. Every call on the main thread. */
internal object LitehtmlNative {
    init {
        System.loadLibrary("twidget_litehtml")
    }

    /** `backend` answers the font, text-metric, image and canvas callbacks for the lifetime of the handle. */
    external fun create(widthDp: Double, dpr: Double, theme: String, backend: LitehtmlBackend): Long
    external fun destroy(handle: Long)
    /** Host → renderer JSON message; returns a JSON array of renderer → host messages. */
    external fun receive(handle: Long, json: String): String
    external fun setWidth(handle: Long, widthDp: Double): String
    external fun setTheme(handle: Long, theme: String): String
    /** action: 0 down, 1 move, 2 up, 3 cancel; coordinates in CSS px. Returns renderer → host messages. */
    external fun touch(handle: Long, action: Int, x: Double, y: Double, pointer: Int, primary: Boolean): String
    /** As touch, from a mouse (no touch events) with modifiers: bit0 alt, bit1 ctrl, bit2 meta, bit3 shift. */
    external fun pointer(handle: Long, action: Int, x: Double, y: Double, pointer: Int, primary: Boolean, mouse: Boolean, modifiers: Int): String
    /** A mouse over the view with no button down (action 0), or leaving it (1). */
    external fun hover(handle: Long, action: Int, x: Double, y: Double, modifiers: Int): String
    /** mode: 0 pixels, 1 lines, 2 pages. */
    external fun wheel(handle: Long, x: Double, y: Double, dx: Double, dy: Double, mode: Int, modifiers: Int): String
    /** type "keydown" or "keyup", with the DOM key and code. */
    external fun key(handle: Long, type: String, key: String, code: String, repeat: Boolean, modifiers: Int): String
    /** One display frame (Choreographer): animations advance. Every frame while wantsTick, else after nextTickIn seconds. */
    external fun tick(handle: Long): String
    external fun wantsTick(handle: Long): Boolean
    external fun nextTickIn(handle: Long): Double
    external fun setCompositing(handle: Long, enabled: Boolean)
    /** The layers above the last displayList (src/compositor.h). */
    external fun composition(handle: Long): String
    /** The renderer clock in seconds: what the layers' track starts are on. */
    external fun clock(): Double
    /** Layer `index` of the last composition as a display list; null past the end. */
    external fun layerContent(handle: Long, index: Int): String?
    /** (width shl 32) or height, CSS px (= dp). */
    external fun size(handle: Long): Long
    external fun dirty(handle: Long): Boolean
    /** Records the document into a display list (JSON, src/display_list.h), CSS px; clears `dirty`. */
    external fun displayList(handle: Long): String
    /** The last rebuild as JSON: serializeMs, parseLayoutMs, passes, htmlBytes, builds, displayOps. */
    external fun stats(handle: Long): String
}
