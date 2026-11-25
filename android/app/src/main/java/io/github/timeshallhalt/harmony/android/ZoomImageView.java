package io.github.timeshallhalt.harmony.android;

import android.content.Context;
import android.graphics.Matrix;
import android.graphics.drawable.Drawable;
import android.os.Bundle;
import android.view.GestureDetector;
import android.view.MotionEvent;
import android.view.ScaleGestureDetector;
import android.view.accessibility.AccessibilityNodeInfo;
import android.widget.ImageView;

/** A fit-centered image that preserves the point beneath a pinch or double tap. */
final class ZoomImageView extends ImageView {
    private static final float MAX_ZOOM = 6f;
    private final Matrix transform = new Matrix();
    private final ScaleGestureDetector scaleDetector;
    private final GestureDetector gestures;
    private float zoom = 1f;
    private float centerX = 0.5f, centerY = 0.5f;
    private float imageWidth, imageHeight, left, top;
    private float lastFocusX, lastFocusY;

    ZoomImageView(Context context) {
        super(context);
        setScaleType(ScaleType.MATRIX);
        setFocusable(true);
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_YES);
        scaleDetector = new ScaleGestureDetector(context, new ScaleGestureDetector.SimpleOnScaleGestureListener() {
            @Override public boolean onScaleBegin(ScaleGestureDetector detector) {
                lastFocusX = detector.getFocusX();
                lastFocusY = detector.getFocusY();
                return imageWidth > 0 && imageHeight > 0;
            }
            @Override public boolean onScale(ScaleGestureDetector detector) {
                float x = detector.getFocusX(), y = detector.getFocusY();
                scaleAt(zoom * detector.getScaleFactor(), lastFocusX, lastFocusY, x, y);
                lastFocusX = x;
                lastFocusY = y;
                return true;
            }
        });
        scaleDetector.setQuickScaleEnabled(false);
        gestures = new GestureDetector(context, new GestureDetector.SimpleOnGestureListener() {
            @Override public boolean onDown(MotionEvent e) { return true; }
            @Override public boolean onScroll(MotionEvent first, MotionEvent e, float dx, float dy) {
                if (e.getPointerCount() == 1 && !scaleDetector.isInProgress() && zoom > 1f && imageWidth > 0 && imageHeight > 0) {
                    centerX += dx / imageWidth;
                    centerY += dy / imageHeight;
                    placeImage();
                }
                return true;
            }
            @Override public boolean onDoubleTap(MotionEvent e) {
                float x = e.getX(), y = e.getY();
                scaleAt(zoom > 1.05f ? 1f : 2.5f, x, y, x, y);
                return true;
            }
            @Override public boolean onSingleTapConfirmed(MotionEvent e) { return performClick(); }
        });
    }

    void show(Drawable drawable) {
        setImageDrawable(drawable);
        placeImage();
    }

    void restorePosition(Bundle state) {
        if (state == null) return;
        zoom = clamp(state.getFloat("image.zoom", 1f), 1f, MAX_ZOOM);
        centerX = clamp(state.getFloat("image.centerX", 0.5f), 0f, 1f);
        centerY = clamp(state.getFloat("image.centerY", 0.5f), 0f, 1f);
    }

    void savePosition(Bundle state) {
        state.putFloat("image.zoom", zoom);
        state.putFloat("image.centerX", centerX);
        state.putFloat("image.centerY", centerY);
    }

    private static float clamp(float value, float min, float max) {
        return Math.max(min, Math.min(max, value));
    }

    private void placeImage() {
        Drawable drawable = getDrawable();
        if (drawable == null || getWidth() <= 0 || getHeight() <= 0) return;
        int w = drawable.getIntrinsicWidth(), h = drawable.getIntrinsicHeight();
        if (w <= 0 || h <= 0) return;
        float fit = Math.min((float) getWidth() / w, (float) getHeight() / h);
        float scale = fit * zoom;
        imageWidth = w * scale;
        imageHeight = h * scale;
        left = offset(getWidth(), imageWidth, centerX);
        top = offset(getHeight(), imageHeight, centerY);
        centerX = (getWidth() / 2f - left) / imageWidth;
        centerY = (getHeight() / 2f - top) / imageHeight;
        transform.setScale(scale, scale);
        transform.postTranslate(left, top);
        setImageMatrix(transform);
    }

    static float offset(float viewSize, float contentSize, float center) {
        if (contentSize <= viewSize) return (viewSize - contentSize) / 2f;
        return clamp(viewSize / 2f - center * contentSize, viewSize - contentSize, 0f);
    }

    private void scaleAt(float requested, float oldX, float oldY, float x, float y) {
        if (imageWidth <= 0 || imageHeight <= 0 || !Float.isFinite(requested)) return;
        float next = clamp(requested, 1f, MAX_ZOOM);
        float ratio = next / zoom;
        centerX = anchoredCenter(oldX, x, left, imageWidth, getWidth(), ratio);
        centerY = anchoredCenter(oldY, y, top, imageHeight, getHeight(), ratio);
        zoom = next;
        placeImage();
    }

    static float anchoredCenter(float before, float after, float start, float size, float viewport, float ratio) {
        float focus = (before - start) / size;
        return focus + (viewport / 2f - after) / (size * ratio);
    }

    @Override protected void onSizeChanged(int w, int h, int oldW, int oldH) {
        super.onSizeChanged(w, h, oldW, oldH);
        placeImage();
    }
    @Override public boolean onTouchEvent(MotionEvent event) {
        scaleDetector.onTouchEvent(event);
        gestures.onTouchEvent(event);
        return true;
    }
    @Override public boolean performClick() {
        super.performClick();
        return true;
    }
    @Override public void onInitializeAccessibilityNodeInfo(AccessibilityNodeInfo info) {
        super.onInitializeAccessibilityNodeInfo(info);
        if (imageWidth <= 0 || imageHeight <= 0) return;
        if (zoom < MAX_ZOOM) {
            info.addAction(new AccessibilityNodeInfo.AccessibilityAction(AccessibilityNodeInfo.ACTION_SCROLL_FORWARD, "Zoom in"));
        }
        if (zoom > 1f) {
            info.addAction(new AccessibilityNodeInfo.AccessibilityAction(AccessibilityNodeInfo.ACTION_SCROLL_BACKWARD, "Zoom out"));
        }
    }
    @Override public boolean performAccessibilityAction(int action, Bundle args) {
        boolean in = action == AccessibilityNodeInfo.ACTION_SCROLL_FORWARD;
        if (imageWidth > 0 && imageHeight > 0 &&
                ((in && zoom < MAX_ZOOM) || (action == AccessibilityNodeInfo.ACTION_SCROLL_BACKWARD && zoom > 1f))) {
            float x = getWidth() / 2f, y = getHeight() / 2f;
            scaleAt(in ? zoom * 2f : zoom / 2f, x, y, x, y);
            return true;
        }
        return super.performAccessibilityAction(action, args);
    }
}
