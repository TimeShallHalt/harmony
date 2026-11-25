//! Android services shared by the Rust shell and Discord's secure store.
use crate::shell::Command;
use anyhow::{Context, Result, anyhow};
use jni21::{
    JNIEnv, JavaVM,
    objects::{GlobalRef, JObject, JString, JValue},
    sys::jlong,
};
use slint::android::AndroidApp;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc::UnboundedSender;
use yapfast_discord::token::SecretStore;

static CURRENT: Mutex<Option<Arc<Platform>>> = Mutex::new(None);
pub struct Platform {
    vm: JavaVM,
    activity: GlobalRef,
    events: UnboundedSender<Command>,
    pub cache: PathBuf,
    pub devices: crate::call::Devices,
}

impl Platform {
    pub fn new(app: &AndroidApp, events: UnboundedSender<Command>) -> Result<Arc<Self>> {
        // NativeActivity owns both pointers until android_main returns. Keep our own global reference.
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast())? };
        let mut env = vm.attach_current_thread()?;
        let borrowed = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
        let activity = env.new_global_ref(&borrowed)?;
        let dir = env
            .call_method(&activity, "getCacheDir", "()Ljava/io/File;", &[])?
            .l()?;
        let path = JString::from(
            env.call_method(dir, "getAbsolutePath", "()Ljava/lang/String;", &[])?
                .l()?,
        );
        let cache = PathBuf::from(String::from(env.get_string(&path)?));
        drop(env);
        let tls_vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr().cast()) };
        tls_vm.attach_current_thread(|env| -> jni::errors::Result<()> {
            let raw = app.activity_as_ptr().cast();
            let activity =
                unsafe { env.as_cast_raw::<jni::refs::Global<jni::objects::JObject>>(&raw)? };
            let context = env
                .call_method(
                    activity,
                    jni::jni_str!("getApplicationContext"),
                    jni::jni_sig!(() -> android.content.Context),
                    &[],
                )?
                .l()?;
            rustls_platform_verifier::android::init_with_env(env, context)
        })?;
        let platform = Arc::new(Self {
            vm,
            activity,
            events,
            cache,
            devices: Default::default(),
        });
        *CURRENT.lock().unwrap() = Some(platform.clone());
        yapfast_discord::token::install(Box::new(Vault));
        Ok(platform)
    }

    fn call<T>(&self, action: impl FnOnce(&mut JNIEnv, &JObject) -> Result<T>) -> Result<T> {
        let mut env = self.vm.attach_current_thread()?;
        env.with_local_frame(16, |env| {
            let result = action(env, self.activity.as_obj());
            if env.exception_check()? {
                env.exception_clear()?;
                return Err(anyhow!("Android could not complete this operation."));
            }
            result
        })
    }
    pub fn simple(&self, method: &str) -> Result<()> {
        self.call(|env, activity| {
            env.call_method(activity, method, "()V", &[])?;
            Ok(())
        })
    }
    pub fn open_discord_qr(&self, epoch: u64, url: &str) -> Result<()> {
        self.call(|env, activity| {
            let url = env.new_string(url)?;
            env.call_method(
                activity,
                "openDiscordQr",
                "(JLjava/lang/String;)V",
                &[(epoch as jlong).into(), (&url).into()],
            )?;
            Ok(())
        })
    }
    pub fn pick(&self, id: i64, limit: i64, media_only: bool) -> Result<()> {
        self.call(|env, a| {
            env.call_method(
                a,
                "pick",
                "(JJZ)V",
                &[id.into(), limit.into(), media_only.into()],
            )?;
            Ok(())
        })
    }
    pub fn notification(&self) -> Result<String> {
        self.call(|env, a| {
            let value = env
                .call_method(a, "takeNotification", "()Ljava/lang/String;", &[])?
                .l()?;
            Ok(String::from(env.get_string(&JString::from(value))?))
        })
    }
    pub fn attach_audio_thread(&self) {
        let _ = self.vm.attach_current_thread_permanently();
    }
    pub fn play(&self, id: i64, pcm: &[i16]) -> Result<()> {
        self.call(|env, a| {
            let samples = env.new_short_array(pcm.len() as i32)?;
            env.set_short_array_region(&samples, 0, pcm)?;
            env.call_method(
                a,
                "playCallAudio",
                "(J[S)V",
                &[id.into(), (&samples).into()],
            )?;
            Ok(())
        })
    }
    pub fn with_id(&self, method: &str, id: i64) -> Result<()> {
        self.call(|env, a| {
            env.call_method(a, method, "(J)V", &[id.into()])?;
            Ok(())
        })
    }
    pub fn record(&self, id: i64) -> Result<()> {
        self.call(|env, a| {
            env.call_method(a, "record", "(J)V", &[id.into()])?;
            Ok(())
        })
    }
    pub fn strings(&self, method: &str, args: &[&str]) -> Result<()> {
        self.call(|env, a| {
            let objects = args
                .iter()
                .map(|s| env.new_string(s).map(JObject::from))
                .collect::<jni21::errors::Result<Vec<_>>>()?;
            let values = objects.iter().map(JValue::Object).collect::<Vec<_>>();
            let sig = format!("({})V", "Ljava/lang/String;".repeat(args.len()));
            env.call_method(a, method, sig.as_str(), &values)?;
            Ok(())
        })
    }
}
fn current() -> Result<Arc<Platform>> {
    CURRENT
        .lock()
        .unwrap()
        .clone()
        .context("Android activity has closed")
}
pub fn detach() {
    CURRENT.lock().unwrap().take();
}
pub(crate) struct Vault;
impl SecretStore for Vault {
    fn read(&self, name: &str) -> Result<Option<String>> {
        current()?.call(|env, a| {
            let key = env.new_string(name)?;
            let value = env
                .call_method(
                    a,
                    "readSecret",
                    "(Ljava/lang/String;)Ljava/lang/String;",
                    &[(&key).into()],
                )?
                .l()?;
            if value.is_null() {
                return Ok(None);
            }
            Ok(Some(env.get_string(&JString::from(value))?.into()))
        })
    }
    fn write(&self, name: &str, value: Option<&str>) -> Result<()> {
        current()?.call(|env, a| {
            let key = env.new_string(name)?;
            let value = match value {
                Some(s) => JObject::from(env.new_string(s)?),
                None => JObject::null(),
            };
            env.call_method(
                a,
                "writeSecret",
                "(Ljava/lang/String;Ljava/lang/String;)V",
                &[(&key).into(), (&value).into()],
            )?;
            Ok(())
        })
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_timeshallhalt_harmony_android_HarmonyActivity_nativeQrUnavailable(
    _: JNIEnv,
    _: jni21::objects::JClass,
    epoch: jlong,
) {
    if let Ok(p) = current() {
        let _ = p.events.send(Command::QrUnavailable(epoch as u64));
    }
}

pub struct StagedFile(pub PathBuf);
impl Drop for StagedFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_timeshallhalt_harmony_android_HarmonyActivity_nativeResult(
    mut env: JNIEnv,
    _class: jni21::objects::JClass,
    id: jlong,
    name: JString,
    path: JString,
    error: JString,
) {
    let text = |e: &mut JNIEnv, s| e.get_string(&s).map(String::from).unwrap_or_default();
    let path = text(&mut env, path);
    let staged = (!path.is_empty()).then(|| StagedFile(path.into()));
    if let Ok(p) = current() {
        let _ = p.events.send(Command::Picked(
            id,
            text(&mut env, name),
            staged,
            text(&mut env, error),
        ));
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_timeshallhalt_harmony_android_HarmonyActivity_microphone(
    env: JNIEnv,
    _: jni21::objects::JClass,
    id: jlong,
    data: jni21::objects::JShortArray,
    n: i32,
) {
    if !(1..=960).contains(&n) {
        return;
    }
    if let Ok(p) = current() {
        let mut pcm = [0i16; 960];
        if env
            .get_short_array_region(&data, 0, &mut pcm[..n as usize])
            .is_ok()
        {
            p.devices.samples(id, &pcm[..n as usize]);
        }
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_timeshallhalt_harmony_android_HarmonyActivity_micState(
    mut env: JNIEnv,
    _: jni21::objects::JClass,
    id: jlong,
    on: jni21::sys::jboolean,
    error: JString,
) {
    if let Ok(p) = current() {
        let reason = env.get_string(&error).map(String::from).unwrap_or_default();
        let _ = p.events.send(Command::MicState(id, on != 0, reason));
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_timeshallhalt_harmony_android_HarmonyActivity_projectionReady(
    _: JNIEnv,
    _: jni21::objects::JClass,
    id: jlong,
    w: i32,
    h: i32,
) {
    if let Ok(p) = current() {
        let _ = p
            .events
            .send(Command::ProjectionReady(id, w as u32, h as u32));
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_timeshallhalt_harmony_android_HarmonyActivity_projectionEnded(
    mut env: JNIEnv,
    _: jni21::objects::JClass,
    id: jlong,
    error: JString,
) {
    if let Ok(p) = current() {
        let reason = env.get_string(&error).map(String::from).unwrap_or_default();
        let _ = p.events.send(Command::ProjectionEnded(id, reason));
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_timeshallhalt_harmony_android_HarmonyActivity_screenFrame(
    env: JNIEnv,
    _: jni21::objects::JClass,
    id: jlong,
    data: jni21::objects::JByteArray,
    time: jlong,
    key: jni21::sys::jboolean,
) -> jni21::sys::jboolean {
    if let (Ok(p), Ok(bytes)) = (current(), env.convert_byte_array(data)) {
        return p.devices.frame(id, bytes, time, key != 0) as u8;
    }
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_timeshallhalt_harmony_android_HarmonyActivity_nativeNotification(
    mut env: JNIEnv,
    _: jni21::objects::JClass,
    channel: JString,
) {
    if let (Ok(p), Ok(id)) = (current(), env.get_string(&channel)) {
        let _ = p.events.send(Command::Notification(id.into()));
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_timeshallhalt_harmony_android_HarmonyActivity_visibility(
    _: JNIEnv,
    _: jni21::objects::JClass,
    visible: jni21::sys::jboolean,
) {
    if let Ok(p) = current() {
        let _ = p.events.send(Command::Visibility(visible != 0));
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_timeshallhalt_harmony_android_HarmonyActivity_nativeBack(
    _: JNIEnv,
    _: jni21::objects::JClass,
) {
    if let Ok(p) = current() {
        let _ = p.events.send(Command::DeviceBack);
    }
}
