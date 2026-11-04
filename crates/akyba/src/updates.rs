//! Updates, through Sparkle: Akyba.app carries `Sparkle.framework` (see `scripts/bundle.sh`),
//! loaded here at launch rather than linked, so a build without it (`cargo run`) simply has no
//! updates. Sparkle checks the newest GitHub release's feed once a day; the feed, the check
//! interval and the public key that updates must be signed with are in the app's Info.plist.
//!
//! A new version never pops up a window on its own. Sparkle downloads it in the background and
//! tells Akyba, which shows "Restart to update" in the status bar; the update is installed then, or
//! when Akyba next quits. Where Sparkle can't download quietly (say Akyba sits somewhere it may not
//! write), the status bar offers the version instead, and clicking it shows Sparkle's window.
//! Checking by hand (Settings → Updates) shows Sparkle's window, as asked.

use std::cell::RefCell;

use block2::{Block, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2::{MainThreadMarker, MainThreadOnly, class, define_class, msg_send};
use objc2_foundation::{NSObject, NSObjectProtocol, NSString};

/// A new version, as the status bar offers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Waiting {
    /// Found, not downloaded: clicking shows Sparkle's window to get it.
    Available(String),
    /// Downloaded: clicking restarts Akyba into it.
    Ready(String),
}

/// The version waiting, if one is, for every window's status bar.
#[derive(Default)]
pub struct Notice(pub Option<Waiting>);

impl gpui_kit::Global for Notice {}

/// The version waiting, if one is.
pub fn waiting(cx: &gpui_kit::App) -> Option<Waiting> {
    cx.try_global::<Notice>().and_then(|notice| notice.0.clone())
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

#[derive(Default)]
struct State {
    /// Sparkle's controller, alive as long as Akyba: it runs the daily checks.
    controller: Option<Retained<AnyObject>>,
    /// Hears from Sparkle what happens to an update; kept alive here, as Sparkle holds it weakly.
    delegate: Option<Retained<UpdateDelegate>>,
    /// Installs the downloaded update and relaunches Akyba.
    install_now: Option<RcBlock<dyn Fn()>>,
    /// Where news of a version goes: the app, which puts it in the status bars.
    news: Option<async_channel::Sender<Option<Waiting>>>,
}

define_class!(
    // Sparkle's updater delegate and its standard user driver's delegate in one: it answers by
    // the method names Sparkle asks for, without declaring the protocols, which Sparkle doesn't
    // require.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "AkybaUpdateDelegate"]
    struct UpdateDelegate;

    unsafe impl NSObjectProtocol for UpdateDelegate {}

    impl UpdateDelegate {
        /// A downloaded update waits to be installed when Akyba quits. Akyba takes it from here:
        /// the status bar offers a restart, so Sparkle needn't remind anyone with a window.
        #[unsafe(method(updater:willInstallUpdateOnQuit:immediateInstallationBlock:))]
        #[expect(unsafe_code, reason = "Sparkle hands over a block to keep, which must be copied")]
        fn will_install_on_quit(&self, _updater: &AnyObject, item: &AnyObject, install: *mut Block<dyn Fn()>) -> Bool {
            let install = unsafe { RcBlock::copy(install) };
            let version = display_version(item);
            STATE.with_borrow_mut(|state| state.install_now = install);
            tell(Some(Waiting::Ready(version)));
            Bool::YES
        }

        /// Sparkle may leave showing a found update to Akyba, as gently as it likes.
        #[unsafe(method(supportsGentleScheduledUpdateReminders))]
        fn supports_gentle_reminders(&self) -> Bool {
            Bool::YES
        }

        /// A scheduled check found an update: never a window of its own, the status bar says so.
        #[unsafe(method(standardUserDriverShouldHandleShowingScheduledUpdate:andInImmediateFocus:))]
        fn should_show_scheduled(&self, _item: &AnyObject, _immediate_focus: Bool) -> Bool {
            Bool::NO
        }

        #[unsafe(method(standardUserDriverWillHandleShowingUpdate:forUpdate:state:))]
        fn will_show(&self, sparkle_shows: Bool, item: &AnyObject, _state: &AnyObject) {
            if !sparkle_shows.as_bool() {
                tell(Some(Waiting::Available(display_version(item))));
            }
        }

        /// The update's window was dealt with (installed, skipped or put off): the status bar's
        /// offer goes, unless one is downloaded and waits for a restart.
        #[unsafe(method(standardUserDriverWillFinishUpdateSession))]
        fn will_finish_session(&self) {
            let downloaded = STATE.with_borrow(|state| state.install_now.is_some());
            if !downloaded {
                tell(None);
            }
        }
    }
);

impl UpdateDelegate {
    #[expect(unsafe_code, reason = "calling NSObject's init on a freshly allocated object is what objc2 asks for")]
    fn new(main_thread: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(main_thread).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

/// The version an appcast item names, as people read it (0.2.4).
#[expect(unsafe_code, reason = "reading a Sparkle appcast item's property")]
fn display_version(item: &AnyObject) -> String {
    let version: *mut NSString = unsafe { msg_send![item, displayVersionString] };
    unsafe { version.as_ref() }.map(ToString::to_string).unwrap_or_default()
}

fn tell(waiting: Option<Waiting>) {
    STATE.with_borrow(|state| {
        if let Some(news) = &state.news {
            let _ = news.try_send(waiting);
        }
    });
}

/// Where news of a version goes, once, at startup.
pub fn on_news(news: async_channel::Sender<Option<Waiting>>) {
    STATE.with_borrow_mut(|state| state.news = Some(news));
}

/// Pretends a version is waiting, for pictures of the status bar (`AKYBA_FAKE_UPDATE=0.2.4`, or
/// `ready:0.2.4` for one downloaded).
pub fn fake(version: &str) {
    tell(Some(match version.strip_prefix("ready:") {
        Some(version) => Waiting::Ready(version.to_owned()),
        None => Waiting::Available(version.to_owned()),
    }));
}

/// Loads Sparkle from the app bundle and starts its updater, downloading new versions quietly.
/// Does nothing outside Akyba.app.
#[expect(unsafe_code, reason = "Sparkle is an Objective-C framework, loaded and called at run time")]
pub fn start() {
    let Some(main_thread) = MainThreadMarker::new() else { return };
    let delegate = UpdateDelegate::new(main_thread);
    let controller = unsafe {
        let bundle: *mut AnyObject = msg_send![class!(NSBundle), mainBundle];
        let frameworks: *mut NSString = msg_send![bundle, privateFrameworksPath];
        let Some(frameworks) = frameworks.as_ref() else { return };
        let path = NSString::from_str(&format!("{frameworks}/Sparkle.framework"));
        let sparkle: *mut AnyObject = msg_send![class!(NSBundle), bundleWithPath: &*path];
        if sparkle.is_null() {
            return;
        }
        let loaded: Bool = msg_send![sparkle, load];
        if !loaded.as_bool() {
            return;
        }
        let Some(controller_class) = AnyClass::get(c"SPUStandardUpdaterController") else { return };
        let allocated: *mut AnyObject = msg_send![controller_class, alloc];
        let delegate_object: &AnyObject = &delegate;
        let controller: *mut AnyObject = msg_send![
            allocated,
            initWithStartingUpdater: Bool::YES,
            updaterDelegate: delegate_object,
            userDriverDelegate: delegate_object
        ];
        let controller = Retained::from_raw(controller);
        if let Some(controller) = &controller {
            // Downloaded in the background, installed on a restart Akyba offers or on quit.
            let updater: *mut AnyObject = msg_send![&**controller, updater];
            if let Some(updater) = updater.as_ref() {
                let () = msg_send![updater, setAutomaticallyDownloadsUpdates: Bool::YES];
            }
        }
        controller
    };
    if std::env::var_os("AKYBA_UPDATES_DEBUG").is_some() {
        eprintln!("updates: Sparkle {}", if controller.is_some() { "running" } else { "not found" });
    }
    STATE.with_borrow_mut(|state| {
        state.controller = controller;
        state.delegate = Some(delegate);
    });
}

/// Checks for a new version now, showing Sparkle's window with the answer. Nothing happens in a
/// development build, which has no Sparkle.
#[expect(unsafe_code, reason = "calling Sparkle's controller")]
pub fn check() {
    STATE.with_borrow(|state| {
        if let Some(controller) = &state.controller {
            let nothing: *mut AnyObject = std::ptr::null_mut();
            let () = unsafe { msg_send![&**controller, checkForUpdates: nothing] };
        }
    });
}

/// Acts on the status bar's offer: restarts into a downloaded version, or shows Sparkle's window
/// for one only found.
pub fn act() {
    let install = STATE.with_borrow(|state| state.install_now.clone());
    match install {
        Some(install) => install.call(()),
        None => check(),
    }
}
