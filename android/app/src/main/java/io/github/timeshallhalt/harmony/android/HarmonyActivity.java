package io.github.timeshallhalt.harmony.android;

import android.Manifest;
import android.app.*;
import android.content.*;
import android.content.pm.PackageManager;
import android.database.Cursor;
import android.media.*;
import android.net.Uri;
import android.media.projection.MediaProjectionManager;
import android.os.*;
import android.provider.OpenableColumns;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;
import android.util.Base64;
import android.view.WindowManager;
import androidx.core.content.FileProvider;
import java.io.*;
import java.security.KeyStore;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import javax.crypto.*;
import javax.crypto.spec.GCMParameterSpec;

public final class HarmonyActivity extends NativeActivity {
    // NativeActivity loads through dlopen. Register the same library with this class loader for JNI callbacks.
    static { System.loadLibrary("harmony_android"); }

    private static final int PICK = 11, RECORD = 12, NOTIFY = 13, CALL_MIC = 14, PROJECT = 15;
    private static final String ALIAS = "harmony.v1";
    private final ExecutorService files = Executors.newSingleThreadExecutor();
    private static final AtomicBoolean processStarted = new AtomicBoolean();
    private CallAudio callAudio;
    private long micPermission, projectionId;
    private static native void visibility(boolean visible);
    private static native void nativeBack();
    private static native void nativeQrUnavailable(long attempt);
    private Runnable unregisterBack;
    private static native void nativeNotification(String channel);
    static native void microphone(long call, short[] pcm, int count);
    static native void micState(long call, boolean enabled, String error);
    static native void projectionReady(long id, int width, int height);
    static native void projectionEnded(long id, String message);
    static native boolean screenFrame(long id, byte[] encoded, long timestamp, boolean keyframe);
    private long pickId, recordId, pickLimit;
    private volatile boolean capturing;
    private volatile boolean visible;
    private static native void nativeResult(long request, String name, String path, String error);

    @Override protected void onCreate(Bundle state) {
        if (processStarted.compareAndSet(false, true)) clearMedia();
        callAudio = new CallAudio(this);
        super.onCreate(state);
        if (Build.VERSION.SDK_INT >= 33) {
            android.window.OnBackInvokedCallback callback = HarmonyActivity::nativeBack;
            getOnBackInvokedDispatcher().registerOnBackInvokedCallback(
                    android.window.OnBackInvokedDispatcher.PRIORITY_DEFAULT, callback);
            unregisterBack = () -> getOnBackInvokedDispatcher().unregisterOnBackInvokedCallback(callback);
        }
        NotificationManager nm = getSystemService(NotificationManager.class);
        nm.createNotificationChannel(new NotificationChannel("messages", "Messages", NotificationManager.IMPORTANCE_DEFAULT));
    }
    public synchronized String takeNotification() {
        Intent intent = getIntent();
        String channel = intent == null ? null : intent.getStringExtra("conversation");
        if (channel == null || !channel.matches("[0-9]{1,20}")) return "";
        intent.removeExtra("conversation");
        return channel;
    }
    @Override protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        String channel = takeNotification();
        if (!channel.isEmpty()) nativeNotification(channel);
    }
    @Override protected void onResume() { super.onResume(); visible = true; visibility(true); }
    @Override protected void onPause() { visible = false; visibility(false); capturing = false; super.onPause(); }
    @Override protected void onStop() { callAudio.suspendMic(); super.onStop(); }
    @Override public void onBackPressed() { nativeBack(); }
    @Override protected void onDestroy() { if (unregisterBack != null) unregisterBack.run(); capturing = false; stopProjection(); callAudio.close(); files.shutdown(); super.onDestroy(); }

    public void openCallAudio(long id) throws IOException { callAudio.open(id); }
    public void playCallAudio(long id, short[] samples) { callAudio.play(id, samples); }
    public void closeCallAudio(long id) { callAudio.close(id); }
    public void muteCallAudio(long id) { callAudio.mute(id); }
    public void requestCallMic(long id) {
        runOnUiThread(() -> {
            if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED) callAudio.mic(id);
            else { micPermission = id; requestPermissions(new String[]{Manifest.permission.RECORD_AUDIO}, CALL_MIC); }
        });
    }
    public void requestProjection(long id) {
        runOnUiThread(() -> {
            projectionId = id;
            try { startActivityForResult(getSystemService(MediaProjectionManager.class).createScreenCaptureIntent(), PROJECT); }
            catch (Exception e) { projectionId = 0; projectionEnded(id, "Screen capture is unavailable."); }
        });
    }
    public void stopProjection() {
        runOnUiThread(() -> { projectionId = 0; stopService(new Intent(this, ProjectionService.class)); });
    }

    private SecretKey key() throws Exception {
        KeyStore ks = KeyStore.getInstance("AndroidKeyStore");
        ks.load(null);
        if (ks.containsAlias(ALIAS)) return (SecretKey)ks.getKey(ALIAS, null);
        KeyGenerator gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore");
        gen.init(new KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT | KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM).setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).build());
        return gen.generateKey();
    }
    public synchronized String readSecret(String name) throws Exception {
        String stored = getSharedPreferences("vault", MODE_PRIVATE).getString(name, null);
        if (stored == null) return null;
        byte[] sealed = Base64.decode(stored, Base64.NO_WRAP);
        Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
        cipher.init(Cipher.DECRYPT_MODE, key(), new GCMParameterSpec(128, sealed, 0, 12));
        cipher.updateAAD(name.getBytes(java.nio.charset.StandardCharsets.UTF_8));
        return new String(cipher.doFinal(sealed, 12, sealed.length - 12), java.nio.charset.StandardCharsets.UTF_8);
    }
    public synchronized void writeSecret(String name, String value) throws Exception {
        SharedPreferences.Editor edit = getSharedPreferences("vault", MODE_PRIVATE).edit();
        if (value == null) edit.remove(name);
        else {
            Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
            cipher.init(Cipher.ENCRYPT_MODE, key());
            cipher.updateAAD(name.getBytes(java.nio.charset.StandardCharsets.UTF_8));
            byte[] encrypted = cipher.doFinal(value.getBytes(java.nio.charset.StandardCharsets.UTF_8));
            ByteArrayOutputStream sealed = new ByteArrayOutputStream();
            sealed.write(cipher.getIV());
            sealed.write(encrypted);
            edit.putString(name, Base64.encodeToString(sealed.toByteArray(), Base64.NO_WRAP));
        }
        if (!edit.commit()) throw new IOException("Could not save encrypted session");
    }

    public void pick(long id, long limit, boolean mediaOnly) {
        runOnUiThread(() -> {
            pickId = id; pickLimit = limit;
            Intent picker = new Intent(Intent.ACTION_OPEN_DOCUMENT).setType("*/*").addCategory(Intent.CATEGORY_OPENABLE);
            if (mediaOnly) picker.putExtra(Intent.EXTRA_MIME_TYPES, new String[]{"image/*", "video/*"});
            try { startActivityForResult(picker, PICK); }
            catch (ActivityNotFoundException e) { nativeResult(id, "", "", "No document picker is available."); }
        });
    }
    @Override protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request == PROJECT) {
            long id = projectionId;
            projectionId = 0;
            if (id == 0) return;
            if (result != RESULT_OK || data == null) { projectionEnded(id, ""); return; }
            try { startForegroundService(new Intent(this, ProjectionService.class).putExtra("id", id).putExtra("consent", data)); }
            catch (Exception e) { projectionEnded(id, "Android declined the screen-sharing service."); }
            return;
        }
        if (request != PICK || pickId == 0) return;
        long id = pickId, limit = pickLimit;
        pickId = 0;
        if (result != RESULT_OK || data == null || data.getData() == null) { nativeResult(id, "", "", ""); return; }
        Uri uri = data.getData();
        files.execute(() -> {
            File temp = null;
            try {
                if (!"content".equals(uri.getScheme())) throw new IOException();
                String name = "attachment";
                try (Cursor row = getContentResolver().query(uri, new String[]{OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE}, null, null, null)) {
                    if (row != null && row.moveToFirst()) {
                        if (!row.isNull(0)) name = row.getString(0);
                        if (!row.isNull(1) && row.getLong(1) > limit) throw new EOFException();
                    }
                }
                temp = File.createTempFile("stage-", ".bin", getCacheDir());
                try (InputStream in = getContentResolver().openInputStream(uri); OutputStream out = new FileOutputStream(temp)) {
                    if (in == null) throw new IOException();
                    byte[] buffer = new byte[65536]; long total = 0; int read;
                    while ((read = in.read(buffer)) != -1) {
                        total += read;
                        if (total > limit) throw new EOFException();
                        out.write(buffer, 0, read);
                    }
                }
                nativeResult(id, name, temp.getAbsolutePath(), "");
                temp = null;
            } catch (Exception e) {
                nativeResult(id, "", "", e instanceof EOFException ? "This file exceeds the upload limit." : "Could not read that document.");
            } finally { if (temp != null) temp.delete(); }
        });
    }
    public void openMedia(String path, String type) {
        runOnUiThread(() -> {
            if (type.startsWith("audio/") || type.startsWith("video/") || type.startsWith("image/")) {
                startActivity(new Intent(this, MediaActivity.class).putExtra("path", path).putExtra("type", type));
            } else {
                try {
                    Uri file = FileProvider.getUriForFile(this, getPackageName() + ".files", new File(path));
                    startActivity(new Intent(Intent.ACTION_VIEW).setDataAndType(file, type).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION));
                } catch (ActivityNotFoundException e) { android.widget.Toast.makeText(this, "No app can open this file.", 0).show(); }
            }
        });
    }
    public void openDiscordQr(long attempt, String url) {
        if (!url.matches("https://discord\\.com/ra/[A-Za-z0-9_-]{1,128}")) {
            nativeQrUnavailable(attempt);
            return;
        }
        runOnUiThread(() -> {
            if (!visible || isFinishing() || isDestroyed()) return;
            Intent intent = new Intent(Intent.ACTION_VIEW, Uri.parse(url))
                    .addCategory(Intent.CATEGORY_BROWSABLE)
                    .setPackage("com.discord");
            try {
                startActivity(intent);
            } catch (ActivityNotFoundException | SecurityException e) {
                nativeQrUnavailable(attempt);
            }
        });
    }

    public void browser(String url) {
        runOnUiThread(() -> {
            try { startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(url))); }
            catch (ActivityNotFoundException e) { android.widget.Toast.makeText(this, "No browser is available.", 0).show(); }
        });
    }
    public void notifications() {
        runOnUiThread(() -> {
            if (Build.VERSION.SDK_INT >= 33 && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED)
                requestPermissions(new String[]{Manifest.permission.POST_NOTIFICATIONS}, NOTIFY);
            else startActivity(new Intent(android.provider.Settings.ACTION_APP_NOTIFICATION_SETTINGS)
                    .putExtra(android.provider.Settings.EXTRA_APP_PACKAGE, getPackageName()));
        });
    }
    public void notifyMessage(String channel, String author, String body) {
        if (visible || !getSystemService(NotificationManager.class).areNotificationsEnabled()) return;
        Intent intent = new Intent(this, HarmonyActivity.class).setFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP | Intent.FLAG_ACTIVITY_CLEAR_TOP)
                .setData(Uri.parse("harmony://conversation/" + channel)).putExtra("conversation", channel);
        PendingIntent open = PendingIntent.getActivity(this, 0, intent, PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        Notification note = new Notification.Builder(this, "messages").setSmallIcon(R.drawable.harmony)
                .setContentTitle(author).setContentText(body).setVisibility(Notification.VISIBILITY_PRIVATE)
                .setAutoCancel(true).setContentIntent(open).build();
        getSystemService(NotificationManager.class).notify(channel, 1, note);
    }
    public void clearMedia() {
        File[] entries = getCacheDir().listFiles();
        if (entries == null) return;
        for (File entry : entries) {
            String name = entry.getName();
            if (name.equals("media") || name.startsWith("stage-") || name.startsWith("voice-")) deleteCached(entry);
        }
    }
    private static void deleteCached(File entry) {
        File[] children = entry.listFiles();
        if (children != null) for (File child : children) deleteCached(child);
        entry.delete();
    }
    public void copyMessage(String text) {
        runOnUiThread(() -> getSystemService(android.content.ClipboardManager.class)
                .setPrimaryClip(android.content.ClipData.newPlainText("Harmony message", text)));
    }
    public void clearNotifications() { getSystemService(NotificationManager.class).cancelAll(); }
    public void record(long id) {
        runOnUiThread(() -> {
            recordId = id;
            if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED)
                requestPermissions(new String[]{Manifest.permission.RECORD_AUDIO}, RECORD);
            else startRecording(id);
        });
    }
    @Override public void onRequestPermissionsResult(int request, String[] permissions, int[] results) {
        super.onRequestPermissionsResult(request, permissions, results);
        if (request == CALL_MIC) {
            long id = micPermission;
            micPermission = 0;
            if (results.length > 0 && results[0] == PackageManager.PERMISSION_GRANTED) callAudio.mic(id);
            else micState(id, false, "Microphone permission was declined.");
        }
        if (request == RECORD && recordId != 0) {
            if (results.length > 0 && results[0] == PackageManager.PERMISSION_GRANTED) startRecording(recordId);
            else nativeResult(recordId, "", "", "Microphone permission was declined.");
        }
    }
    public void stopRecording() { recordId = 0; capturing = false; }
    private void startRecording(long id) {
        capturing = true;
        files.execute(() -> {
            AudioRecord audio = null; File temp = null;
            try {
                int size = AudioRecord.getMinBufferSize(48000, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT);
                audio = new AudioRecord(MediaRecorder.AudioSource.VOICE_RECOGNITION, 48000, AudioFormat.CHANNEL_IN_MONO,
                        AudioFormat.ENCODING_PCM_16BIT, Math.max(7680, size));
                temp = File.createTempFile("voice-", ".pcm", getCacheDir());
                audio.startRecording();
                if (audio.getRecordingState() != AudioRecord.RECORDSTATE_RECORDING) throw new IOException();
                try (OutputStream out = new FileOutputStream(temp)) {
                    byte[] block = new byte[1920]; long bytes = 0;
                    while (capturing && bytes < 48000L * 2 * 20 * 60) {
                        int n = audio.read(block, 0, block.length);
                        if (n <= 0) throw new IOException();
                        out.write(block, 0, n); bytes += n;
                    }
                }
                nativeResult(id, "Voice message", temp.getAbsolutePath(), ""); temp = null;
            } catch (Exception e) { nativeResult(id, "", "", "Could not capture microphone audio."); }
            finally { capturing = false; if (audio != null) audio.release(); if (temp != null) temp.delete(); }
        });
    }
}
