//! Animated emoji's frames, decoded ahead of the draw: what is wanted next,
//! the frames kept once they arrive, and the byte budget they are kept within.
//!
//! Message text draws an emoji from a glyph its layout made, so its frames
//! cannot be decoded while drawing without stalling the frame. The rows a
//! refresh builds name the animated emoji on screen instead, and those go to
//! the front of the queue while a worker elsewhere decodes them at the size
//! jumbo text draws. This module is only the bookkeeping around that, with no
//! thread, no timer and no window, so it is tested on its own.

use super::*;

#[cfg(all(test, feature = "discord"))]
#[path = "update4_emoji_acceptance_tests.rs"]
mod acceptance;

/// Ten frames a second, for a frame whose animation gave it no delay. A GIF's
/// delays are read with its pixels, so this only covers a picture that lost
/// them.
const DEFAULT_DELAY: u32 = 100;

/// One animated emoji's decoded frames, ready for the registry.
///
/// The images share the pixel buffers the decoder filled, so Slint and the
/// renderer draw rather than copy them, and the bytes counted here are what
/// they cost whoever holds them.
pub(super) struct Frames {
    images: Vec<slint::Image>,
    delays: Vec<u32>,
    bytes: usize,
}

impl Frames {
    /// `frames` are RGBA, decoded at the size the rows draw them; `delays`
    /// are milliseconds.
    ///
    /// A frame the delays do not reach takes the last delay given, so every
    /// image has one and the clock walks the frames one by one.
    pub(super) fn new(
        frames: Vec<slint::SharedPixelBuffer<slint::Rgba8Pixel>>,
        delays: &[u32],
    ) -> Self {
        let bytes: usize = frames.iter().map(|frame| frame.as_bytes().len()).sum();
        let images: Vec<slint::Image> = frames.into_iter().map(slint::Image::from_rgba8).collect();
        let delays: Vec<u32> = (0..images.len())
            .map(|index| {
                delays
                    .get(index)
                    .copied()
                    .or(delays.last().copied())
                    .unwrap_or(DEFAULT_DELAY)
            })
            .collect();
        Self {
            images,
            delays,
            bytes,
        }
    }

    /// What the frames cost, as the pixels they are made of.
    pub(super) fn bytes(&self) -> usize {
        self.bytes
    }

    /// The frames themselves, in the order they play.
    pub(super) fn images(&self) -> &[slint::Image] {
        &self.images
    }

    /// How long each frame shows, in milliseconds and in the same order.
    pub(super) fn delays(&self) -> &[u32] {
        &self.delays
    }
}

/// The emoji whose frames are wanted, visible ones first.
pub(super) struct Job {
    /// The name the picker and the registry know the emoji by.
    pub key: String,
    /// The glyph message text draws it as, if the emoji is also in the font.
    pub glyph: Option<char>,
    /// The picture to decode, which is what makes two jobs one job.
    pub address: String,
}

/// Wanted frames, deduplicated by picture address, visible first.
///
/// Nothing here starts a decode; the worker takes jobs as it can, and a job
/// still waiting when the refresh no longer wants it is taken and dropped.
#[derive(Default)]
pub(super) struct Queue {
    waiting: Vec<Job>,
}

impl Queue {
    /// Wants an emoji's frames: at the back behind what is already waiting, or
    /// at the front when it was waiting already.
    ///
    /// The front is for a refresh naming the rows on screen again, which it
    /// does every time, so what is on screen is decoded before what is left.
    /// One emoji posted in forty messages is one job, by its picture's
    /// address, and one name is one job, by its key: the frames decoded once
    /// serve every row that draws them.
    ///
    /// A key whose frames failed is never passed here. The caller asks
    /// [`Cache::is_still`] first, because a picture that could not be read
    /// would fail the same way on every refresh.
    pub(super) fn want(&mut self, job: Job) {
        let waiting = self
            .waiting
            .iter()
            .position(|waiting| waiting.address == job.address || waiting.key == job.key);
        if let Some(index) = waiting {
            // The newest address for an emoji wins: its picture is decoded for
            // the name the rows just asked for. Moving it in front is what a
            // refresh naming its rows again amounts to.
            self.waiting.remove(index);
            self.waiting.insert(0, job);
        } else {
            self.waiting.push(job);
        }
    }

    /// Takes the emoji to decode next, which is the one wanted most recently.
    pub(super) fn take(&mut self) -> Option<Job> {
        if self.waiting.is_empty() {
            None
        } else {
            Some(self.waiting.remove(0))
        }
    }

    /// Forgets what was wanted, as everything wanted changed at once: another
    /// channel, or a frame size that leaves every decoded frame too small.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn clear(&mut self) {
        self.waiting.clear();
    }

    /// How many emoji are waiting to be decoded.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn len(&self) -> usize {
        self.waiting.len()
    }
}

/// Frames kept, by emoji key, within a byte budget.
pub(super) struct Cache {
    entries: HashMap<String, Kept>,
    /// Counts every showing, so the smallest count is the emoji shown longest
    /// ago: the whole of a least-recently-shown cache.
    shown: u64,
    /// The emoji a refresh showed, which eviction leaves alone.
    pinned: HashSet<String>,
    /// The emoji whose frames could not be decoded, so they are not asked for
    /// again on every refresh.
    stills: HashSet<String>,
    bytes: usize,
}

/// One emoji's frames, and when they last showed.
struct Kept {
    frames: Rc<Frames>,
    glyph: Option<char>,
    shown: u64,
}

impl Cache {
    pub(super) const BUDGET: usize = 6 * 1024 * 1024;

    pub(super) fn new() -> Self {
        Self {
            entries: HashMap::new(),
            shown: 0,
            pinned: HashSet::new(),
            stills: HashSet::new(),
            bytes: 0,
        }
    }

    /// Keeps frames that just arrived, shown from now on. Frames for a key
    /// already here replace them, which is what a new frame size amounts to.
    pub(super) fn insert(&mut self, key: String, glyph: Option<char>, frames: Rc<Frames>) {
        self.shown += 1;
        let shown = self.shown;
        self.bytes += frames.bytes();
        self.stills.remove(&key);
        let replaced = self.entries.insert(
            key,
            Kept {
                frames,
                glyph,
                shown,
            },
        );
        if let Some(replaced) = replaced {
            self.bytes -= replaced.frames.bytes();
        }
    }

    /// The frames kept for `key`, if any, from now the last thing shown.
    pub(super) fn get(&mut self, key: &str) -> Option<Rc<Frames>> {
        self.shown += 1;
        let shown = self.shown;
        let kept = self.entries.get_mut(key)?;
        kept.shown = shown;
        Some(kept.frames.clone())
    }

    /// Pins what a refresh just showed, so eviction never takes it.
    ///
    /// Every refresh pins the whole visible set, so this replaces the pinned
    /// set rather than adding to it. A pinned set larger than the budget
    /// stays: an emoji dropping back to its still while it is on screen is
    /// worse than the memory, and it is what a budget alone would do.
    pub(super) fn pin<'a>(&mut self, keys: impl IntoIterator<Item = &'a str>) {
        self.pinned.clear();
        self.pinned.extend(keys.into_iter().map(str::to_owned));
    }

    /// Drops the frames shown longest ago until the budget is met again,
    /// never one pinned.
    pub(super) fn evict(&mut self) {
        self.evict_to(Self::BUDGET);
    }

    /// The budget is a constant, so the tests lower it rather than fill
    /// megabytes of pictures to make eviction happen.
    fn evict_to(&mut self, budget: usize) {
        while self.bytes > budget {
            let Some(key) = self.longest_unshown() else {
                // Only pinned emoji are left, and they are never taken.
                return;
            };
            if let Some(taken) = self.entries.remove(&key) {
                i_slint_core::yapfast_emoji::remove(&key);
                self.bytes -= taken.frames.bytes();
            }
        }
    }

    /// The key shown longest ago, of those not pinned.
    fn longest_unshown(&self) -> Option<String> {
        self.entries
            .iter()
            .filter(|(key, _)| !self.pinned.contains(*key))
            .min_by_key(|(_, kept)| kept.shown)
            .map(|(key, _)| key.clone())
    }

    /// What the kept frames cost, for a report and for the tests.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn bytes(&self) -> usize {
        self.bytes
    }

    /// Whether frames are kept for `key`, without counting as a showing.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn contains(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    /// The glyph an emoji's frames draw as, for a pass that registers what is
    /// kept once more, as a new frame size makes it do.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn glyph(&self, key: &str) -> Option<char> {
        self.entries.get(key).and_then(|kept| kept.glyph)
    }

    /// Remembers that an emoji's frames could not be decoded, so every refresh
    /// until they do is spared asking again.
    pub(super) fn still(&mut self, key: &str) {
        self.stills.insert(key.to_owned());
    }

    /// Whether an emoji's frames are known not to decode. A still costs no
    /// bytes and is not evicted: the pictures it names are few, and forgetting
    /// one would only retry it.
    pub(super) fn is_still(&self, key: &str) -> bool {
        self.stills.contains(key)
    }
}

/// One emoji's frames on their way back from the decoder.
struct Decoded {
    key: String,
    glyph: Option<char>,
    frames: Vec<Vec<u8>>,
    delays: Vec<u32>,
}

/// How many decodes run at once: one, so the rows and the picker keep their
/// cores and the emoji a refresh shows next do not queue behind a long one.
const DECODING_AT_ONCE: usize = 1;

/// The animated emoji on screen: what they need decoded, what is kept, and the
/// clock they all draw from.
pub(super) struct Animations {
    picker: HashSet<String>,
    cache: Cache,
    queue: Queue,
    /// The emoji whose frames a thread is decoding now.
    decoding: std::collections::HashSet<String>,
    done: std::sync::mpsc::Sender<Decoded>,
    decoded: std::sync::mpsc::Receiver<Decoded>,
    /// The side frames decode at: 48px as jumbo text draws them, for the
    /// screen's scale, and never past 96.
    side: u32,
    /// Moves the clock while anything plays; stopped otherwise, so a window
    /// with no animated emoji on screen has no wakeups.
    timer: slint::Timer,
    /// How often the clock moves, for the fastest animation on screen.
    interval: u64,
}

impl Drop for Animations {
    fn drop(&mut self) {
        self.timer.stop();
        for key in self.cache.entries.keys() {
            i_slint_core::yapfast_emoji::remove(key);
        }
    }
}

impl Animations {
    pub(super) fn new() -> Self {
        let (done, decoded) = std::sync::mpsc::channel();
        Self {
            cache: Cache::new(),
            picker: HashSet::new(),
            queue: Queue::default(),
            decoding: std::collections::HashSet::new(),
            done,
            decoded,
            side: 48,
            timer: slint::Timer::default(),
            interval: 0,
        }
    }

    /// Names the animated emoji a refresh shows: those kept are pinned so
    /// eviction leaves them, and those missing are wanted for decoding.
    pub(super) fn show(
        &mut self,
        shown: impl IntoIterator<Item = (String, Option<char>, String)>,
        pictures: &HashMap<String, Arc<Picture>>,
    ) {
        let mut keys: Vec<String> = Vec::new();
        let mut wanted: Vec<(String, Option<char>, String)> = Vec::new();
        for (key, glyph, address) in shown {
            // The font may have been made after the frames were kept, or made
            // again since, so run of text that draws this emoji is named here.
            if let Some(glyph) = glyph {
                if let Some(kept) = self.cache.entries.get_mut(&key) {
                    kept.glyph = Some(glyph);
                }
                i_slint_core::yapfast_emoji::set_glyph(&key, glyph);
            }
            if self.cache.get(&key).is_some()
                || self.cache.is_still(&key)
                || self.decoding.contains(&key)
            {
                keys.push(key);
                continue;
            }
            if !pictures.contains_key(&address) {
                // Its picture has not arrived; the still shows, and the next
                // refresh wants it again once it has.
                keys.push(key);
                continue;
            }
            keys.push(key.clone());
            wanted.push((key, glyph, address));
        }
        self.cache.pin(keys.iter().map(String::as_str));
        self.reconcile_registry();
        self.queue
            .waiting
            .retain(|job| self.cache.pinned.contains(&job.key) || self.picker.contains(&job.key));
        for (key, glyph, address) in wanted {
            self.queue.want(Job {
                key,
                glyph,
                address,
            });
        }
    }

    /// Wants frames for emoji shown somewhere that pins nothing, such as the
    /// picker's visible cells: they scroll away, so they are decoded in turn
    /// but never hold another emoji's place in the cache.
    pub(super) fn also(
        &mut self,
        shown: impl IntoIterator<Item = (String, Option<char>, String)>,
        pictures: &HashMap<String, Arc<Picture>>,
    ) {
        let shown: Vec<_> = shown.into_iter().collect();
        self.picker = shown.iter().map(|(key, _, _)| key.clone()).collect();
        self.queue
            .waiting
            .retain(|job| self.cache.pinned.contains(&job.key) || self.picker.contains(&job.key));
        self.reconcile_registry();
        for (key, glyph, address) in shown {
            if self.cache.contains(&key)
                || self.cache.is_still(&key)
                || self.decoding.contains(&key)
                || !pictures.contains_key(&address)
            {
                continue;
            }
            self.queue.want(Job {
                key,
                glyph,
                address,
            });
        }
    }

    /// Starts decoding what is wanted, within the budget of work in flight.
    fn reconcile_registry(&self) {
        for (key, kept) in &self.cache.entries {
            if self.cache.pinned.contains(key) || self.picker.contains(key) {
                if i_slint_core::yapfast_emoji::frame_of_key(key).is_none() {
                    i_slint_core::yapfast_emoji::insert(
                        key,
                        kept.glyph,
                        Rc::new(i_slint_core::yapfast_emoji::Animation::new(
                            kept.frames.images().to_vec(),
                            kept.frames.delays(),
                        )),
                    );
                }
            } else {
                i_slint_core::yapfast_emoji::remove(key);
            }
        }
    }

    pub(super) fn start(&mut self, pictures: &HashMap<String, Arc<Picture>>) {
        while self.decoding.len() < DECODING_AT_ONCE {
            let Some(job) = self.queue.take() else {
                break;
            };
            let Some(picture) = pictures.get(&job.address) else {
                continue;
            };
            let (picture, side, done) = (picture.clone(), self.side, self.done.clone());
            self.decoding.insert(job.key.clone());
            std::thread::spawn(move || {
                let decoded = decode(&picture, side);
                let _ = done.send(Decoded {
                    key: job.key,
                    glyph: job.glyph,
                    frames: decoded.0,
                    delays: decoded.1,
                });
            });
        }
    }

    /// Keeps the frames that arrived, and gives the registry each emoji's
    /// animation, so text and the picker draw them from now on.
    pub(super) fn drain(&mut self) {
        while let Ok(decoded) = self.decoded.try_recv() {
            self.decoding.remove(&decoded.key);
            if decoded.frames.is_empty() {
                self.cache.still(&decoded.key);
                continue;
            }
            let mut buffers = Vec::with_capacity(decoded.frames.len());
            for frame in decoded.frames {
                let mut buffer =
                    slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(self.side, self.side);
                let bytes = buffer.make_mut_bytes();
                let count = bytes.len().min(frame.len());
                bytes[..count].copy_from_slice(&frame[..count]);
                buffers.push(buffer);
            }
            let frames = Rc::new(Frames::new(buffers, &decoded.delays));
            let animation = Rc::new(i_slint_core::yapfast_emoji::Animation::new(
                frames.images().to_vec(),
                frames.delays(),
            ));
            if self.cache.pinned.contains(&decoded.key) || self.picker.contains(&decoded.key) {
                i_slint_core::yapfast_emoji::insert(&decoded.key, decoded.glyph, animation);
            }
            self.cache.insert(decoded.key, decoded.glyph, frames);
            self.cache.evict();
        }
    }

    /// Takes what arrived from the decoders, starts what is wanted next, and
    /// draws what shows a frame again. Called each tick and each refresh, so a
    /// decode that finishes is seen at once rather than at the next change.
    pub(super) fn run(&mut self, window: &YapfastWindow, pictures: &HashMap<String, Arc<Picture>>) {
        if window.global::<Motion>().get_playing() {
            self.start(pictures);
        }
        self.drain();
        window
            .global::<EmojiAnimations>()
            .set_revision(i_slint_core::yapfast_emoji::revision() as i32);
        window.window().request_redraw();
    }

    /// Sets whether the clock may move, and starts or stops the timer: it runs
    /// while anything animates, or while frames are on their way, and it is
    /// the clock itself that holds still when playback is off.
    pub(super) fn sync(
        &mut self,
        window: &YapfastWindow,
        playing: bool,
        adapter: std::rc::Weak<RefCell<Adapter>>,
    ) {
        i_slint_core::yapfast_emoji::set_playing(playing);
        self.side = frame_side(window.window().scale_factor());
        let waiting = !self.decoding.is_empty()
            || (playing && (self.queue.len() > 0 || !i_slint_core::yapfast_emoji::is_empty()));
        if !waiting {
            self.timer.stop();
            self.interval = 0;
            return;
        }
        // A frame no faster than 16ms and no slower than 50 keeps the clock off
        // a busy loop while still following a slow animation's own pace; with
        // nothing registered yet, it only looks for what arrives.
        let delay = i_slint_core::yapfast_emoji::shortest_delay().unwrap_or(30);
        let interval = u64::from(delay.clamp(16, 50));
        if self.timer.running() && self.interval == interval {
            return;
        }
        self.interval = interval;
        let (weak, clock) = (
            window.as_weak(),
            Rc::new(Cell::new(std::time::Instant::now())),
        );
        self.timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(interval),
            move || {
                let now = std::time::Instant::now();
                let delta = now.saturating_duration_since(clock.replace(now));
                i_slint_core::yapfast_emoji::advance(delta);
                let (Some(adapter), Some(window)) = (adapter.upgrade(), weak.upgrade()) else {
                    return;
                };
                let owner = Rc::downgrade(&adapter);
                let mut adapter = adapter.borrow_mut();
                let Adapter {
                    app, animations, ..
                } = &mut *adapter;
                animations.run(&window, &app.state.pictures);
                // Completion can be the last outstanding work while playback is
                // paused. Reconcile here instead of waiting for another refresh.
                animations.sync(&window, i_slint_core::yapfast_emoji::playing(), owner);
            },
        );
    }
}

/// The side frames decode at: 48px, as jumbo text draws an emoji, for the
/// screen's scale, and never past the 96 that keeps the cache's budget useful.
fn frame_side(scale: f32) -> u32 {
    (48.0 * scale.max(1.0)).round().clamp(48.0, 96.0) as u32
}

/// An animation's frames at `side`, and how long each shows, decoded whole and
/// scaled at once: the picture's own frames are as large as its file.
fn decode(picture: &Picture, side: u32) -> (Vec<Vec<u8>>, Vec<u32>) {
    use image::imageops;
    let delays = picture
        .animation
        .as_ref()
        .map(|animation| animation.delays.clone())
        .unwrap_or_default();
    let frames = picture
        .frames()
        .filter_map(|frame| {
            let decoded = image::RgbaImage::from_raw(picture.width, picture.height, frame)?;
            let scaled = imageops::resize(
                &crate::emoji_text::premultiplied(&decoded),
                side,
                side,
                imageops::FilterType::Triangle,
            );
            Some(crate::emoji_text::unpremultiplied(&scaled).into_raw())
        })
        .collect();
    (frames, delays)
}

#[test]
fn frames_reach_the_registry() {
    use yapfast_core::chat;
    // A GIF of two frames, as a Discord emoji's picture is.
    let mut data = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut data);
        for index in 0..2u8 {
            let shade = image::Rgba([index.wrapping_mul(200), 0, 0, 255]);
            let frame = image::Frame::from_parts(
                image::RgbaImage::from_pixel(8, 8, shade),
                0,
                0,
                image::Delay::from_numer_denom_ms(50, 1),
            );
            encoder.encode_frame(frame).unwrap();
        }
    }
    let picture = Arc::new(chat::decode_picture(&data, 128, 128).expect("a picture"));
    assert!(picture.animation.is_some(), "a GIF animates");

    let (key, glyph) = ("frames-reach:1", '\u{10ffe1}');
    let mut animations = Animations::new();
    animations.side = 16;
    let pictures = HashMap::from([("pic".to_owned(), picture)]);
    animations.show(
        vec![(key.to_owned(), Some(glyph), "pic".to_owned())],
        &pictures,
    );
    animations.start(&pictures);
    // The worker decodes on its own thread; the tick waits for it the same way.
    for _ in 0..200 {
        animations.drain();
        if i_slint_core::yapfast_emoji::frame_of_glyph(glyph).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        i_slint_core::yapfast_emoji::frame_of_glyph(glyph).is_some(),
        "the frames reached the registry"
    );
    i_slint_core::yapfast_emoji::remove(key);
}

#[test]
fn update4_generated_frames_retire_registry_and_timer_with_actual_adapter() {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let window = YapfastWindow::new().unwrap();
    let owner = Adapter::new(
        Arc::new(yapfast_core::runtime::BackgroundRuntime::new().unwrap()),
        dir.path().into(),
        &window,
    )
    .unwrap();
    let mut data = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut data);
        for (red, delay) in [(11, 40), (231, 90)] {
            encoder
                .encode_frame(image::Frame::from_parts(
                    image::RgbaImage::from_pixel(4, 4, image::Rgba([red, 31, 71, 255])),
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(delay, 1),
                ))
                .unwrap();
        }
    }
    let pic = Arc::new(yapfast_core::chat::decode_picture(&data, 128, 128).unwrap());
    let pictures = HashMap::from([("generated".into(), pic)]);
    let key = "update4-generated:200";
    {
        let mut a = owner.borrow_mut();
        a.animations
            .show(vec![(key.into(), None, "generated".into())], &pictures);
        a.animations.start(&pictures);
        for _ in 0..200 {
            a.animations.drain();
            if i_slint_core::yapfast_emoji::frame_of_key(key).is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    i_slint_core::yapfast_emoji::set_playing(true);
    let first = i_slint_core::yapfast_emoji::frame_of_key(key).unwrap();
    assert_eq!(
        &first.to_rgba8().unwrap().as_bytes()[..4],
        &[11, 31, 71, 255]
    );
    i_slint_core::yapfast_emoji::advance(std::time::Duration::from_millis(40));
    let second = i_slint_core::yapfast_emoji::frame_of_key(key).unwrap();
    assert_eq!(
        &second.to_rgba8().unwrap().as_bytes()[..4],
        &[231, 31, 71, 255]
    );
    drop(first);
    drop(second);
    let mut a = owner.borrow_mut();
    a.animations.show(Vec::new(), &pictures);
    a.animations.sync(&window, true, Rc::downgrade(&owner));
    assert!(
        i_slint_core::yapfast_emoji::frame_of_key(key).is_none(),
        "no consumer retains a registry animation"
    );
    assert!(
        !a.animations.timer.running(),
        "no idle animation timer after consumers leave"
    );
    a.animations
        .show(vec![(key.into(), None, "generated".into())], &pictures);
    assert!(
        i_slint_core::yapfast_emoji::frame_of_key(key).is_some(),
        "cache reopens without another decode"
    );
    let weak = Rc::downgrade(&a.animations.cache.get(key).unwrap());
    drop(a);
    drop(owner);
    assert!(weak.upgrade().is_none());
    assert!(
        i_slint_core::yapfast_emoji::frame_of_key(key).is_none(),
        "dropping actual Adapter retires owned registry entries"
    );
}

#[test]
fn update4_paused_decoded_emoji_quiesces_and_resumes_actual_adapter_timer() {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let window = YapfastWindow::new().unwrap();
    let owner = Adapter::new(
        Arc::new(yapfast_core::runtime::BackgroundRuntime::new().unwrap()),
        dir.path().into(),
        &window,
    )
    .unwrap();
    let mut data = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut data);
        for (red, delay) in [(17, 40), (223, 90)] {
            encoder
                .encode_frame(image::Frame::from_parts(
                    image::RgbaImage::from_pixel(4, 4, image::Rgba([red, 31, 71, 255])),
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(delay, 1),
                ))
                .unwrap();
        }
    }
    let picture = Arc::new(yapfast_core::chat::decode_picture(&data, 128, 128).unwrap());
    let pictures = HashMap::from([("generated-paused".into(), picture)]);
    let key = "update4-paused:201";
    let mut a = owner.borrow_mut();
    a.animations.show(
        vec![(key.into(), None, "generated-paused".into())],
        &pictures,
    );
    a.animations.start(&pictures);
    for _ in 0..200 {
        a.animations.drain();
        if a.animations.decoding.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(a.animations.decoding.is_empty());
    assert_eq!(a.animations.queue.len(), 0);
    a.animations.sync(&window, true, Rc::downgrade(&owner));
    assert!(
        a.animations.timer.running(),
        "positive control: visible playing emoji ticks"
    );
    let first = i_slint_core::yapfast_emoji::frame_of_key(key).unwrap();
    a.animations.sync(&window, false, Rc::downgrade(&owner));
    assert!(
        !a.animations.timer.running(),
        "paused decoded emoji must not retain an idle wakeup timer"
    );
    i_slint_core::yapfast_emoji::advance(std::time::Duration::from_millis(100));
    assert_eq!(
        i_slint_core::yapfast_emoji::frame_of_key(key).unwrap(),
        first
    );
    a.animations.sync(&window, true, Rc::downgrade(&owner));
    assert!(a.animations.timer.running());
    i_slint_core::yapfast_emoji::advance(std::time::Duration::from_millis(40));
    assert_eq!(
        &i_slint_core::yapfast_emoji::frame_of_key(key)
            .unwrap()
            .to_rgba8()
            .unwrap()
            .as_bytes()[..4],
        &[223, 31, 71, 255]
    );
    a.animations.show(Vec::new(), &pictures);
    a.animations.sync(&window, true, Rc::downgrade(&owner));
    assert!(!a.animations.timer.running());
}

#[test]
fn update4_paused_inflight_decode_stops_timer_after_completion_without_refresh() {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let window = YapfastWindow::new().unwrap();
    let owner = Adapter::new(
        Arc::new(yapfast_core::runtime::BackgroundRuntime::new().unwrap()),
        dir.path().into(),
        &window,
    )
    .unwrap();
    let mut data = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut data);
        for red in [19, 221] {
            encoder
                .encode_frame(image::Frame::from_parts(
                    image::RgbaImage::from_pixel(4, 4, image::Rgba([red, 31, 71, 255])),
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(40, 1),
                ))
                .unwrap();
        }
    }
    let key = "update4-paused-inflight:202";
    {
        let mut a = owner.borrow_mut();
        a.app.state.pictures.insert(
            "inflight".into(),
            Arc::new(yapfast_core::chat::decode_picture(&data, 128, 128).unwrap()),
        );
        let Adapter {
            app, animations, ..
        } = &mut *a;
        animations.show(
            vec![(key.into(), None, "inflight".into())],
            &app.state.pictures,
        );
        animations.start(&app.state.pictures);
        assert_eq!(
            animations.decoding.len(),
            1,
            "completion still awaits the real timer consumer"
        );
        animations.sync(&window, false, Rc::downgrade(&owner));
        assert!(
            animations.timer.running(),
            "bounded completion polling remains live"
        );
    }
    // The owning adapter can pause for inactivity even when the global mode
    // still permits motion. A completion must preserve that stricter policy.
    window.global::<Motion>().set_playing(true);
    for _ in 0..200 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(50));
        slint::platform::update_timers_and_animations();
        if owner.borrow().animations.decoding.is_empty() {
            break;
        }
    }
    let a = owner.borrow();
    assert!(
        a.animations.decoding.is_empty(),
        "real timer must consume generated decode"
    );
    assert!(
        i_slint_core::yapfast_emoji::frame_of_key(key).is_some(),
        "still fallback remains available"
    );
    assert!(
        !a.animations.timer.running(),
        "paused completion must quiesce without a later app refresh"
    );
}

#[test]
fn update4_paused_actual_adapter_defers_new_frame_decode_until_playback_resumes() {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let window = YapfastWindow::new().unwrap();
    let owner = Adapter::new(
        Arc::new(yapfast_core::runtime::BackgroundRuntime::new().unwrap()),
        dir.path().into(),
        &window,
    )
    .unwrap();
    let mut data = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut data);
        for red in [23, 219] {
            encoder
                .encode_frame(image::Frame::from_parts(
                    image::RgbaImage::from_pixel(4, 4, image::Rgba([red, 31, 71, 255])),
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(40, 1),
                ))
                .unwrap();
        }
    }
    let key = "update4-paused-admission:203";
    let mut a = owner.borrow_mut();
    a.app.state.pictures.insert(
        "paused-admission".into(),
        Arc::new(yapfast_core::chat::decode_picture(&data, 128, 128).unwrap()),
    );
    let Adapter {
        app, animations, ..
    } = &mut *a;
    animations.show(
        vec![(key.into(), None, "paused-admission".into())],
        &app.state.pictures,
    );
    assert_eq!(
        animations.queue.len(),
        1,
        "real received source wants animation frames"
    );
    window.global::<Motion>().set_playing(false);
    animations.run(&window, &app.state.pictures);
    animations.sync(&window, false, Rc::downgrade(&owner));
    assert!(
        animations.decoding.is_empty(),
        "paused real run must not admit a later frame decoder"
    );
    assert_eq!(
        animations.queue.len(),
        1,
        "deferred work survives without decode"
    );
    assert!(
        !animations.timer.running(),
        "deferred paused queue must not poll"
    );
    window.global::<Motion>().set_playing(true);
    animations.run(&window, &app.state.pictures);
    assert_eq!(
        animations.decoding.len(),
        1,
        "resume admits exactly the deferred production decode"
    );
    for _ in 0..200 {
        animations.drain();
        if animations.decoding.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(animations.decoding.is_empty());
    assert!(i_slint_core::yapfast_emoji::frame_of_key(key).is_some());
}

#[test]
fn repair1_picker_no_match_and_known_empty_retire_consumers_without_refresh() {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let window = YapfastWindow::new().unwrap();
    let owner = Adapter::new(
        Arc::new(yapfast_core::runtime::BackgroundRuntime::new().unwrap()),
        dir.path().into(),
        &window,
    )
    .unwrap();
    bind_callbacks(&window, &owner);
    window.set_emoji_panel_width(498.);
    window.set_react_item("fixture-picker".into());
    window.global::<Motion>().set_playing(true);
    for empty_viewport in [false, true] {
        window.invoke_emoji_search("pizza".into());
        let key = "repair1-picker:210";
        let weak;
        {
            let mut a = owner.borrow_mut();
            let frames = Rc::new(Frames::new(
                vec![
                    slint::SharedPixelBuffer::new(2, 2),
                    slint::SharedPixelBuffer::new(2, 2),
                ],
                &[40, 90],
            ));
            weak = Rc::downgrade(&frames);
            a.animations.cache.insert(key.into(), None, frames);
            a.animations
                .also(vec![(key.into(), None, "missing".into())], &HashMap::new());
            a.animations.sync(&window, true, Rc::downgrade(&owner));
            assert!(a.animations.timer.running());
        }
        if empty_viewport {
            window.invoke_emoji_scrolled_to(100000., 50.);
        } else {
            window.invoke_emoji_search("no-such-emoji-repair1-xyz".into());
        }
        {
            let mut a = owner.borrow_mut();
            assert!(
                a.animations.picker.is_empty(),
                "known empty picker must replace its previous consumers"
            );
            assert!(i_slint_core::yapfast_emoji::frame_of_key(key).is_none());
            assert!(
                !a.animations.timer.running(),
                "model/viewport replacement must reconcile timer immediately"
            );
            // Ordinary production budget pressure, without forced eviction/purge.
            for i in 0..4 {
                a.animations.cache.insert(
                    format!("pressure:{i}"),
                    None,
                    Rc::new(Frames::new(
                        vec![slint::SharedPixelBuffer::new(1024, 1024)],
                        &[40],
                    )),
                );
                a.animations.cache.evict();
            }
        }
        assert!(weak.upgrade().is_none());
        window.invoke_emoji_search("pizza".into());
        assert!(window.get_emoji_rows().row_count() > 0);
    }
}

#[test]
fn repair1_preference_event_resumes_quiesced_timer_without_refresh() {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let window = YapfastWindow::new().unwrap();
    let owner = Adapter::new(
        Arc::new(yapfast_core::runtime::BackgroundRuntime::new().unwrap()),
        dir.path().into(),
        &window,
    )
    .unwrap();
    bind_callbacks(&window, &owner);
    ANIMATE.set(Animate::Always);
    let key = "repair1-event:209";
    let mut data = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut data);
        for red in [37, 199] {
            encoder
                .encode_frame(image::Frame::from_parts(
                    image::RgbaImage::from_pixel(4, 4, image::Rgba([red, 31, 71, 255])),
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(40, 1),
                ))
                .unwrap();
        }
    }
    {
        let mut a = owner.borrow_mut();
        a.app.state.pictures.insert(
            "event-generated".into(),
            Arc::new(yapfast_core::chat::decode_picture(&data, 128, 128).unwrap()),
        );
        let Adapter {
            app, animations, ..
        } = &mut *a;
        animations.show(
            vec![(key.into(), None, "event-generated".into())],
            &app.state.pictures,
        );
        animations.start(&app.state.pictures);
        for _ in 0..200 {
            animations.drain();
            if animations.decoding.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        animations.sync(&window, true, Rc::downgrade(&owner));
        assert!(animations.timer.running());
    }
    // Only the received backend preference changes. No direct show_motion,
    // run, sync or Application refresh is allowed after this initial setup.
    i_slint_core::with_existing_context(|context| {
        context.set_motion_preference(i_slint_core::MotionPreference::Reduced)
    })
    .unwrap();
    i_slint_core::properties::ChangeTracker::run_change_handlers();
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(50));
    slint::platform::update_timers_and_animations();
    assert!(!owner.borrow().animations.timer.running());
    let before = i_slint_core::yapfast_emoji::revision();
    i_slint_core::with_existing_context(|context| {
        context.set_motion_preference(i_slint_core::MotionPreference::NoPreference)
    })
    .unwrap();
    i_slint_core::properties::ChangeTracker::run_change_handlers();
    assert!(
        owner.borrow().animations.timer.running(),
        "preference event must restart the stopped owning timer"
    );
    std::thread::sleep(Duration::from_millis(45));
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(50));
    slint::platform::update_timers_and_animations();
    assert!(i_slint_core::yapfast_emoji::revision() > before);
    assert_eq!(
        &i_slint_core::yapfast_emoji::frame_of_key(key)
            .unwrap()
            .to_rgba8()
            .unwrap()
            .as_bytes()[..4],
        &[199, 31, 71, 255]
    );
}

#[test]
fn repair1_retained_adapter_detach_retires_decoded_and_inflight_animation() {
    i_slint_backend_testing::init_no_event_loop();
    for inflight in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let window = YapfastWindow::new().unwrap();
        let owner = Adapter::new(
            Arc::new(yapfast_core::runtime::BackgroundRuntime::new().unwrap()),
            dir.path().into(),
            &window,
        )
        .unwrap();
        let mut data = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut data);
            for red in [29, 211] {
                encoder
                    .encode_frame(image::Frame::from_parts(
                        image::RgbaImage::from_pixel(4, 4, image::Rgba([red, 31, 71, 255])),
                        0,
                        0,
                        image::Delay::from_numer_denom_ms(40, 1),
                    ))
                    .unwrap();
            }
        }
        let pictures = HashMap::from([(
            "detach-generated".into(),
            Arc::new(yapfast_core::chat::decode_picture(&data, 128, 128).unwrap()),
        )]);
        let key = "repair1-detach:208";
        let weak_frames;
        {
            let mut a = owner.borrow_mut();
            a.animations.show(
                vec![(key.into(), None, "detach-generated".into())],
                &pictures,
            );
            a.animations.also(
                vec![(key.into(), None, "detach-generated".into())],
                &pictures,
            );
            a.animations.start(&pictures);
            if !inflight {
                for _ in 0..200 {
                    a.animations.drain();
                    if a.animations.decoding.is_empty() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                weak_frames = Some(Rc::downgrade(&a.animations.cache.get(key).unwrap()));
            } else {
                weak_frames = None;
            }
            a.animations.sync(&window, true, Rc::downgrade(&owner));
            assert!(a.animations.timer.running());
            a.detach();
            assert!(
                !a.animations.timer.running(),
                "tray-retained Adapter must stop animation wakeups immediately"
            );
            assert!(a.animations.cache.pinned.is_empty() && a.animations.picker.is_empty());
            assert!(i_slint_core::yapfast_emoji::frame_of_key(key).is_none());
        }
        drop(window);
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(5));
            i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(50));
            slint::platform::update_timers_and_animations();
        }
        assert!(weak_frames.is_none_or(|weak| weak.upgrade().is_none()));
        let next = YapfastWindow::new().unwrap();
        {
            let mut a = owner.borrow_mut();
            a.window = next.as_weak();
            a.animations.drain();
            assert!(
                a.animations.cache.entries.is_empty(),
                "late old decoder cannot revive retired window"
            );
            a.animations.show(
                vec![(key.into(), None, "detach-generated".into())],
                &pictures,
            );
            a.animations.start(&pictures);
            for _ in 0..200 {
                a.animations.drain();
                if a.animations.decoding.is_empty() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            a.animations.sync(&next, true, Rc::downgrade(&owner));
            assert!(a.animations.timer.running());
            assert!(i_slint_core::yapfast_emoji::frame_of_key(key).is_some());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A decoded picture as the decoders hand them over, `side` pixels a side.
    fn frame(side: u32) -> slint::SharedPixelBuffer<slint::Rgba8Pixel> {
        slint::SharedPixelBuffer::new(side, side)
    }

    /// One emoji's frames: `count` pictures of 2 by 2 pixels, 16 bytes each,
    /// small enough that a test fills a budget of its own choosing.
    fn small(count: usize) -> Rc<Frames> {
        let delays = vec![40; count];
        Rc::new(Frames::new((0..count).map(|_| frame(2)).collect(), &delays))
    }

    fn job(key: &str, address: &str) -> Job {
        Job {
            key: key.to_owned(),
            glyph: Some('a'),
            address: address.to_owned(),
        }
    }

    #[test]
    fn repair1_retired_cached_frames_bind_current_glyph_on_first_show() {
        for old in [None, Some('\u{10ffe3}')] {
            let key = "repair1-glyph:207";
            let glyph = '\u{10ffe4}';
            let mut animations = Animations::new();
            animations.cache.insert(key.into(), old, small(2));
            animations.also(vec![(key.into(), old, "missing".into())], &HashMap::new());
            assert!(i_slint_core::yapfast_emoji::frame_of_key(key).is_some());
            animations.also(Vec::new(), &HashMap::new());
            assert!(i_slint_core::yapfast_emoji::frame_of_key(key).is_none());
            animations.show(
                vec![(key.into(), Some(glyph), "missing".into())],
                &HashMap::new(),
            );
            assert_eq!(
                i_slint_core::yapfast_emoji::frame_of_glyph(glyph),
                i_slint_core::yapfast_emoji::frame_of_key(key),
                "first reopened text consumer must bind its current glyph"
            );
            assert!(i_slint_core::yapfast_emoji::frame_of_glyph(glyph).is_some());
        }
    }

    #[test]
    fn one_picture_is_decoded_once_however_many_rows_draw_it() {
        let mut queue = Queue::default();
        queue.want(job("party:1", "https://cdn/1.gif"));
        queue.want(job("party:1", "https://cdn/1.gif"));
        assert_eq!(queue.len(), 1, "the same emoji posted twice");
        queue.want(job("wave:2", "https://cdn/1.gif"));
        assert_eq!(queue.len(), 1, "one picture, whatever it is called");
        assert_eq!(queue.take().map(|job| job.key).as_deref(), Some("wave:2"));
        assert_eq!(queue.len(), 0, "taken");
        assert!(queue.take().is_none(), "nothing left to take");
    }

    #[test]
    fn re_wanting_an_emoji_moves_it_ahead_of_the_rest() {
        let mut queue = Queue::default();
        queue.want(job("a", "1"));
        queue.want(job("b", "2"));
        queue.want(job("c", "3"));
        // A refresh names the rows on screen again, and they are decoded first.
        queue.want(job("c", "3"));
        let order: Vec<String> = std::iter::from_fn(|| queue.take())
            .map(|job| job.key)
            .collect();
        assert_eq!(order, ["c", "a", "b"]);
        queue.want(job("d", "4"));
        queue.clear();
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn eviction_takes_the_frames_shown_longest_ago() {
        let mut cache = Cache::new();
        for key in ["a", "b", "c"] {
            cache.insert(key.to_owned(), Some('a'), small(1));
        }
        assert_eq!(cache.bytes(), 48, "three frames of 2 by 2 pixels");
        // Showing a and c again leaves b the one not shown for longest.
        assert!(cache.get("a").is_some());
        assert!(cache.get("c").is_some());
        cache.evict_to(32);
        assert!(!cache.contains("b"), "shown longest ago");
        assert!(cache.contains("a") && cache.contains("c"));
        assert_eq!(cache.bytes(), 32);
        cache.evict_to(16);
        assert!(!cache.contains("a"), "shown before c");
        assert!(cache.contains("c"), "shown last, so kept");
        assert_eq!(cache.bytes(), 16);
    }

    #[test]
    fn a_pinned_emoji_stays_even_over_the_budget() {
        let mut cache = Cache::new();
        cache.insert("a".to_owned(), Some('a'), small(1));
        cache.insert("b".to_owned(), Some('b'), small(1));
        cache.pin(["b"]);
        cache.evict_to(0);
        assert!(cache.contains("b"), "on screen, so it keeps its frames");
        assert!(!cache.contains("a"));
        assert_eq!(cache.bytes(), 16, "over the budget, and pinned");
        // A refresh pins what it showed, so the pinned set replaces the last.
        cache.pin(["a"]);
        cache.evict_to(0);
        assert!(!cache.contains("b"));
        assert_eq!(cache.bytes(), 0);
    }

    #[test]
    fn bytes_are_the_pixels_kept_and_replacing_them_counts_once() {
        assert_eq!(Frames::new(vec![frame(4)], &[40]).bytes(), 64);
        let mut cache = Cache::new();
        assert_eq!(cache.bytes(), 0);
        cache.insert("a".to_owned(), Some('a'), small(3));
        assert_eq!(cache.bytes(), 48);
        cache.insert("a".to_owned(), Some('a'), small(1));
        assert_eq!(cache.bytes(), 16, "replaced rather than added");
        assert!(cache.get("a").is_some());
        assert_eq!(cache.bytes(), 16, "showing costs nothing");
        assert_eq!(cache.glyph("a"), Some('a'));
        assert_eq!(cache.glyph("gone"), None);
    }

    #[test]
    fn an_emoji_that_failed_stays_still_until_frames_arrive() {
        let mut cache = Cache::new();
        assert!(!cache.is_still("party:1"), "not known to fail yet");
        cache.still("party:1");
        assert!(cache.is_still("party:1"), "the caller skips wanting it");
        assert_eq!(cache.bytes(), 0, "a still is remembered, not kept");
        // Frames that arrive on a later try are what it draws from then on.
        cache.insert("party:1".to_owned(), Some('a'), small(1));
        assert!(!cache.is_still("party:1"));
    }

    #[test]
    fn every_image_has_a_delay_to_show_for() {
        let frames = Frames::new(vec![frame(2), frame(2), frame(2)], &[40, 60]);
        assert_eq!(frames.images().len(), 3);
        assert_eq!(frames.delays(), &[40, 60, 60], "the last delay repeats");
        let nothing = Frames::new(vec![frame(2)], &[]);
        assert_eq!(nothing.delays(), &[DEFAULT_DELAY]);
        let more = Frames::new(vec![frame(2)], &[40, 60]);
        assert_eq!(more.delays(), &[40], "a frame there is not, is not kept");
    }
}
