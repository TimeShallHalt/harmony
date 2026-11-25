package io.github.timeshallhalt.harmony.android;

import android.app.Activity;
import android.content.ClipData;
import android.content.Intent;
import android.net.Uri;
import android.text.TextUtils;
import androidx.core.content.FileProvider;
import android.graphics.ImageDecoder;
import android.graphics.drawable.AnimatedImageDrawable;
import android.graphics.drawable.Drawable;
import android.media.MediaPlayer;
import android.os.Bundle;
import android.view.Gravity;
import android.widget.*;
import java.io.File;
import java.io.IOException;
import java.util.Locale;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public final class MediaActivity extends Activity {
    private ZoomImageView image;
    private AnimatedImageDrawable animation;
    private ExecutorService imageLoader;
    private boolean resumed;
    private VideoView video;
    private MediaPlayer audio;
    private boolean audioReady;
    private final android.os.Handler clock = new android.os.Handler(android.os.Looper.getMainLooper());

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        String path = getIntent().getStringExtra("path");
        String type = getIntent().getStringExtra("type");
        if (path == null || type == null) { finish(); return; }
        File media;
        try {
            File directory = new File(getCacheDir(), "media").getCanonicalFile();
            media = new File(path).getCanonicalFile();
            if (!media.toPath().startsWith(directory.toPath()) || !media.isFile()) {
                unsupported();
                return;
            }
        } catch (IOException e) { unsupported(); return; }
        LinearLayout page = new LinearLayout(this);
        page.setOrientation(LinearLayout.VERTICAL);
        page.setBackgroundColor(0xff111113);
        LinearLayout top = new LinearLayout(this);
        top.setGravity(Gravity.CENTER_VERTICAL);
        int unit = Math.round(4 * getResources().getDisplayMetrics().density);
        int target = 12 * unit;
        top.setPadding(unit, unit, unit, unit);
        TextView close = new TextView(this);
        close.setText("×");
        close.setTextColor(0xfff2f2f6);
        close.setTextSize(30);
        close.setGravity(Gravity.CENTER);
        close.setContentDescription("Close attachment");
        close.setFocusable(true);
        close.setOnClickListener(v -> finish());
        top.addView(close, target, target);
        TextView title = new TextView(this);
        title.setText(media.getName());
        title.setTextColor(0xfff2f2f6);
        title.setTextSize(16);
        title.setSingleLine(true);
        title.setEllipsize(TextUtils.TruncateAt.END);
        title.setPadding(unit, 0, unit, 0);
        top.addView(title, new LinearLayout.LayoutParams(0, -2, 1));
        ImageButton share = new ImageButton(this);
        share.setImageResource(android.R.drawable.ic_menu_share);
        share.setColorFilter(0xffdadbe3);
        share.setBackgroundColor(android.graphics.Color.TRANSPARENT);
        share.setContentDescription("Share attachment");
        share.setOnClickListener(v -> {
            Uri uri = FileProvider.getUriForFile(this, getPackageName() + ".files", media);
            Intent send = new Intent(Intent.ACTION_SEND).setType(type).putExtra(Intent.EXTRA_STREAM, uri);
            send.setClipData(ClipData.newRawUri(media.getName(), uri));
            send.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
            startActivity(Intent.createChooser(send, null));
        });
        top.addView(share, target, target);
        page.addView(top);
        FrameLayout frame = new FrameLayout(this);
        page.addView(frame, new LinearLayout.LayoutParams(-1, 0, 1));
        page.setOnApplyWindowInsetsListener((v, insets) -> {
            v.setPadding(insets.getSystemWindowInsetLeft(), insets.getSystemWindowInsetTop(),
                    insets.getSystemWindowInsetRight(), insets.getSystemWindowInsetBottom());
            return insets;
        });
        setContentView(page);
        if (type.startsWith("image/")) {
            openImage(media, frame, state);
        } else if (type.startsWith("audio/")) {
            openAudio(media.getPath(), frame, 3 * unit);
        } else {
            video = new VideoView(this);
            frame.addView(video, new FrameLayout.LayoutParams(-1, -2, Gravity.CENTER));
            MediaController controls = new MediaController(this);
            video.setMediaController(controls); controls.setAnchorView(video);
            video.setVideoPath(media.getPath());
            video.setOnPreparedListener(p -> { video.start(); controls.show(); });
            video.setOnErrorListener((p, what, extra) -> { unsupported(); return true; });
        }
    }
    private void openImage(File file, FrameLayout frame, Bundle state) {
        if (file.length() > 32L * 1024 * 1024) { unsupported(); return; }
        image = new ZoomImageView(this);
        image.setContentDescription(file.getName());
        image.restorePosition(state);
        frame.addView(image, -1, -1);
        imageLoader = Executors.newSingleThreadExecutor();
        imageLoader.execute(() -> {
            try {
                Drawable result = ImageDecoder.decodeDrawable(ImageDecoder.createSource(file), (decoder, info, source) -> {
                    int w = info.getSize().getWidth(), h = info.getSize().getHeight();
                    double scale = decodeScale(w, h, info.isAnimated());
                    if (scale < 1) {
                        decoder.setTargetSize(Math.max(1, (int) (w * scale)), Math.max(1, (int) (h * scale)));
                    }
                });
                runOnUiThread(() -> {
                    if (isFinishing() || isDestroyed()) return;
                    image.show(result);
                    if (result instanceof AnimatedImageDrawable) {
                        animation = (AnimatedImageDrawable) result;
                        if (resumed) animation.start();
                    }
                });
            } catch (IOException | IllegalArgumentException | OutOfMemoryError e) {
                runOnUiThread(() -> { if (!isFinishing() && !isDestroyed()) unsupported(); });
            }
        });
        imageLoader.shutdown();
    }

    static double decodeScale(int width, int height, boolean animated) {
        long pixels = (long) width * height;
        // Animated decoders may composite frames at the original canvas size.
        if (width <= 0 || height <= 0 || (animated && pixels > 16_777_216L)) {
            throw new IllegalArgumentException("Unsupported image canvas");
        }
        int edge = animated ? 1536 : 3072;
        long budget = animated ? 2_097_152L : 6_291_456L;
        return Math.min(1, Math.min((double) edge / Math.max(width, height), Math.sqrt((double) budget / pixels)));
    }

    private void openAudio(String path, FrameLayout frame, int pad) {
        LinearLayout controls = new LinearLayout(this);
        controls.setOrientation(LinearLayout.VERTICAL); controls.setPadding(pad, 0, pad, 0);
        frame.addView(controls, new FrameLayout.LayoutParams(-1, -2, Gravity.CENTER));
        TextView time = new TextView(this); time.setGravity(Gravity.CENTER); controls.addView(time);
        SeekBar seek = new SeekBar(this); seek.setContentDescription("Playback position"); controls.addView(seek);
        Button toggle = new Button(this); toggle.setText("Play"); toggle.setEnabled(false); controls.addView(toggle);
        audio = new MediaPlayer();
        audio.setOnErrorListener((player, what, extra) -> { unsupported(); return true; });
        audio.setOnPreparedListener(player -> {
            audioReady = true; seek.setMax(Math.max(1,player.getDuration())); toggle.setEnabled(true);
            if (!isFinishing() && hasWindowFocus()) player.start();
            clock.post(new Runnable() {
                @Override public void run() {
                    if (audio == null || !audioReady) return;
                    seek.setProgress(audio.getCurrentPosition());
                    time.setText(stamp(audio.getCurrentPosition()) + " / " + stamp(seek.getMax()));
                    toggle.setText(audio.isPlaying() ? "Pause" : "Play");
                    clock.postDelayed(this, 250);
                }
            });
        });
        toggle.setOnClickListener(v -> { if (audio.isPlaying()) audio.pause(); else audio.start(); });
        seek.setOnSeekBarChangeListener(new SeekBar.OnSeekBarChangeListener() {
            public void onStartTrackingTouch(SeekBar bar) {}
            public void onStopTrackingTouch(SeekBar bar) {}
            public void onProgressChanged(SeekBar bar, int value, boolean fromUser) {
                if (fromUser && audioReady) audio.seekTo(value);
            }
        });
        try { audio.setDataSource(path); audio.prepareAsync(); } catch (IOException e) { unsupported(); }
    }
    private static String stamp(int milliseconds) {
        return String.format(Locale.ROOT, "%d:%02d", milliseconds / 60000, (milliseconds / 1000) % 60);
    }
    private void unsupported() { Toast.makeText(this, "Unsupported media.", Toast.LENGTH_SHORT).show(); finish(); }
    @Override protected void onSaveInstanceState(Bundle state) {
        super.onSaveInstanceState(state);
        if (image != null) image.savePosition(state);
    }
    @Override protected void onResume() {
        super.onResume();
        resumed = true;
        if (animation != null) animation.start();
    }
    @Override protected void onPause() {
        resumed = false;
        if (animation != null) animation.stop();
        super.onPause();
    }
    @Override protected void onStop() {
        if (video != null) video.pause();
        if (audioReady && audio.isPlaying()) audio.pause();
        super.onStop();
    }
    @Override protected void onDestroy() {
        if (imageLoader != null) imageLoader.shutdownNow();
        if (animation != null) { animation.stop(); animation = null; }
        if (image != null) image.setImageDrawable(null);
        clock.removeCallbacksAndMessages(null);
        if (audio != null) { audioReady = false; audio.release(); audio = null; }
        if (video != null) video.stopPlayback();
        super.onDestroy();
    }
}
