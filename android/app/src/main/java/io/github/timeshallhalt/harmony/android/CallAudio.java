package io.github.timeshallhalt.harmony.android;

import android.content.Context;
import android.media.*;
import android.os.Process;
import java.io.IOException;

/** Android owns the audio devices. Rust owns the voice connection, Opus and the mute state. */
final class CallAudio {
    private final AudioManager manager;
    private AudioTrack speaker;
    private AudioFocusRequest focus;
    private volatile long session;
    private volatile int captureGeneration;

    CallAudio(Context context) { manager = context.getSystemService(AudioManager.class); }

    synchronized void open(long id) throws IOException {
        if (session != 0) close(session);
        AudioAttributes attributes = new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_VOICE_COMMUNICATION)
                .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH).build();
        focus = new AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT)
                .setAudioAttributes(attributes).setOnAudioFocusChangeListener(change -> {
                    if (change < 0) suspendMic();
                }).build();
        session = id;
        try {
            if (manager.requestAudioFocus(focus) != AudioManager.AUDIOFOCUS_REQUEST_GRANTED)
                throw new IOException("Audio focus unavailable");
            manager.setMode(AudioManager.MODE_IN_COMMUNICATION);
            AudioFormat format = new AudioFormat.Builder().setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                    .setSampleRate(48000).setChannelMask(AudioFormat.CHANNEL_OUT_STEREO).build();
            int min = AudioTrack.getMinBufferSize(48000, AudioFormat.CHANNEL_OUT_STEREO, AudioFormat.ENCODING_PCM_16BIT);
            speaker = new AudioTrack.Builder().setAudioAttributes(attributes).setAudioFormat(format)
                    .setBufferSizeInBytes(Math.max(15360, min)).setTransferMode(AudioTrack.MODE_STREAM).build();
            speaker.play();
        } catch (Exception e) { close(id); throw new IOException("Could not open communication audio", e); }
    }
    synchronized void play(long id, short[] pcm) {
        if (session == id && speaker != null) speaker.write(pcm, 0, pcm.length, AudioTrack.WRITE_NON_BLOCKING);
    }
    synchronized void mic(long id) {
        if (id == 0 || id != session) return;
        final int generation = ++captureGeneration;
        new Thread(() -> {
            Process.setThreadPriority(Process.THREAD_PRIORITY_AUDIO);
            AudioRecord record = null;
            try {
                int min = AudioRecord.getMinBufferSize(48000, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT);
                record = new AudioRecord(MediaRecorder.AudioSource.VOICE_COMMUNICATION, 48000, AudioFormat.CHANNEL_IN_MONO,
                        AudioFormat.ENCODING_PCM_16BIT, Math.max(7680, min));
                record.startRecording();
                if (record.getRecordingState() != AudioRecord.RECORDSTATE_RECORDING) throw new IOException();
                if (session != id || generation != captureGeneration) return;
                HarmonyActivity.micState(id, true, "");
                short[] frame = new short[960];
                while (session == id && generation == captureGeneration) {
                    int n = record.read(frame, 0, frame.length, AudioRecord.READ_BLOCKING);
                    if (n <= 0) throw new IOException();
                    if (generation == captureGeneration) HarmonyActivity.microphone(id, frame, n);
                }
            } catch (Exception e) {
                if (session == id && generation == captureGeneration) HarmonyActivity.micState(id, false, "Could not open the call microphone.");
            } finally { if (record != null) record.release(); }
        }, "Harmony microphone").start();
    }
    synchronized void mute(long id) { if (id == session) ++captureGeneration; }
    synchronized void suspendMic() {
        if (session == 0) return;
        mute(session);
        HarmonyActivity.micState(session, false, "");
    }
    synchronized void close(long id) {
        if (id != session) return;
        mute(id);
        session = 0;
        if (speaker != null) { speaker.release(); speaker = null; }
        if (focus != null) { manager.abandonAudioFocusRequest(focus); focus = null; }
        manager.setMode(AudioManager.MODE_NORMAL);
    }
    synchronized void close() { if (session != 0) close(session); }
}
