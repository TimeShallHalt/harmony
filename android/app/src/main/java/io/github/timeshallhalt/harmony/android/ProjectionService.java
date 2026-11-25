package io.github.timeshallhalt.harmony.android;

import android.app.*;
import android.content.*;
import android.content.pm.ServiceInfo;
import android.graphics.Point;
import android.hardware.display.*;
import android.media.*;
import android.media.projection.*;
import android.os.*;
import android.view.*;
import java.io.ByteArrayOutputStream;
import java.nio.ByteBuffer;

/** The projection token stays in this foreground service for exactly one consent session. */
public final class ProjectionService extends Service {
    private static final String STOP = "stop";
    private volatile boolean active;
    private HandlerThread worker;
    private long id;

    @Override public android.os.IBinder onBind(Intent intent) { return null; }
    @Override public int onStartCommand(Intent start, int flags, int startId) {
        if (start == null || STOP.equals(start.getAction())) { stopSelf(); return START_NOT_STICKY; }
        if (worker != null) return START_NOT_STICKY;
        id = start.getLongExtra("id", 0);
        NotificationManager nm = getSystemService(NotificationManager.class);
        nm.createNotificationChannel(new NotificationChannel("projection", "Screen sharing", NotificationManager.IMPORTANCE_LOW));
        PendingIntent stop = PendingIntent.getService(this, 0, new Intent(this, ProjectionService.class).setAction(STOP),
                PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        Notification notification = new Notification.Builder(this, "projection")
                .setSmallIcon(R.drawable.harmony).setContentTitle("Harmony is sharing your screen").setOngoing(true)
                .addAction(new Notification.Action.Builder(null, "Stop sharing", stop).build()).build();
        if (Build.VERSION.SDK_INT >= 29) startForeground(41, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION);
        else startForeground(41, notification);
        Intent consent = start.getParcelableExtra("consent");
        active = true;
        worker = new HandlerThread("Harmony screen encoder");
        worker.start();
        new Handler(worker.getLooper()).post(() -> encode(consent));
        return START_NOT_STICKY;
    }
    @SuppressWarnings("deprecation")
    private void encode(Intent consent) {
        MediaProjection projection = null;
        MediaCodec encoder = null;
        Surface surface = null;
        VirtualDisplay display = null;
        String error = "";
        try {
            projection = getSystemService(MediaProjectionManager.class).getMediaProjection(Activity.RESULT_OK, consent);
            if (projection == null) throw new IllegalStateException();
            // The codec drains on the worker, so revocation must be delivered on a separate looper.
            projection.registerCallback(new MediaProjection.Callback() {
                @Override public void onStop() { active = false; }
            }, new Handler(Looper.getMainLooper()));
            Point size = new Point();
            getSystemService(WindowManager.class).getDefaultDisplay().getRealSize(size);
            double scale = Math.min(1.0, 1280.0 / Math.max(size.x, size.y));
            int w = Math.max(16, ((int)(size.x * scale) / 16) * 16);
            int h = Math.max(16, ((int)(size.y * scale) / 16) * 16);
            MediaFormat format = MediaFormat.createVideoFormat(MediaFormat.MIMETYPE_VIDEO_AVC, w, h);
            format.setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface);
            format.setInteger(MediaFormat.KEY_BIT_RATE, 1600000);
            format.setInteger(MediaFormat.KEY_FRAME_RATE, 15);
            format.setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 2);
            format.setLong(MediaFormat.KEY_REPEAT_PREVIOUS_FRAME_AFTER, 200000L);
            format.setFloat(MediaFormat.KEY_MAX_FPS_TO_ENCODER, 15f);
            encoder = MediaCodec.createEncoderByType(MediaFormat.MIMETYPE_VIDEO_AVC);
            encoder.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE);
            surface = encoder.createInputSurface();
            encoder.start();
            display = projection.createVirtualDisplay("Harmony Go Live", w, h, getResources().getDisplayMetrics().densityDpi,
                    DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR, surface, null, null);
            if (display == null) throw new IllegalStateException();
            HarmonyActivity.projectionReady(id, w, h);
            MediaCodec.BufferInfo info = new MediaCodec.BufferInfo();
            byte[] config = new byte[0];
            while (active) {
                int index = encoder.dequeueOutputBuffer(info, 50000);
                if (index == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED) {
                    MediaFormat output = encoder.getOutputFormat();
                    ByteArrayOutputStream headers = new ByteArrayOutputStream();
                    for (String key : new String[]{"csd-0", "csd-1"}) {
                        ByteBuffer bytes = output.getByteBuffer(key);
                        if (bytes != null) { byte[] part = new byte[bytes.remaining()]; bytes.get(part); headers.write(part); }
                    }
                    config = headers.toByteArray();
                }
                if (index < 0) continue;
                try {
                    ByteBuffer output = encoder.getOutputBuffer(index);
                    if (output == null || info.size == 0) continue;
                    output.position(info.offset); output.limit(info.offset + info.size);
                    byte[] raw = new byte[info.size];
                    output.get(raw);
                    if ((info.flags & MediaCodec.BUFFER_FLAG_CODEC_CONFIG) != 0) { config = raw; continue; }
                    boolean key = (info.flags & MediaCodec.BUFFER_FLAG_KEY_FRAME) != 0;
                    ByteArrayOutputStream frame = new ByteArrayOutputStream(raw.length + config.length);
                    if (key) appendAnnexB(frame, config);
                    appendAnnexB(frame, raw);
                    if (HarmonyActivity.screenFrame(id, frame.toByteArray(), info.presentationTimeUs, key)) {
                        Bundle request = new Bundle();
                        request.putInt(MediaCodec.PARAMETER_KEY_REQUEST_SYNC_FRAME, 0);
                        encoder.setParameters(request);
                    }
                } finally { encoder.releaseOutputBuffer(index, false); }
            }
        } catch (Exception e) { error = "Android could not share this display (" + e.getClass().getSimpleName() + ")."; }
        finally {
            if (display != null) display.release();
            if (encoder != null) encoder.release();
            if (surface != null) surface.release();
            if (projection != null) projection.stop();
            HarmonyActivity.projectionEnded(id, error);
            worker.quitSafely();
            stopSelf();
        }
    }
    private static void appendAnnexB(ByteArrayOutputStream out, byte[] bytes) throws java.io.IOException {
        if (bytes.length == 0) return;
        if (bytes.length >= 4 && bytes[0] == 0 && bytes[1] == 0 &&
                (bytes[2] == 1 || (bytes[2] == 0 && bytes[3] == 1))) { out.write(bytes); return; }
        ByteBuffer input = ByteBuffer.wrap(bytes);
        while (input.remaining() >= 4) {
            int length = input.getInt();
            if (length <= 0 || length > input.remaining()) throw new java.io.IOException("Invalid AVC frame");
            out.write(new byte[]{0, 0, 0, 1});
            out.write(bytes, input.position(), length);
            input.position(input.position() + length);
        }
    }
    @Override public void onDestroy() { active = false; stopForeground(STOP_FOREGROUND_REMOVE); super.onDestroy(); }
}
