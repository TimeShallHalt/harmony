//! A resizable harness importing the production chrome, not copying its hit maps.
// Match the production .slint files' legacy edition until they are migrated.
#![allow(deprecated)]
slint::slint! {
    import { TitleBar, ResizeEdges } from "../../ui/title_bar.slint";
    import { WindowBar } from "../../ui/yapfast/window-bar.slint";
    import { ClientStyle } from "../../ui/yapfast/style.slint";
    export { ClientStyle }
    export component ChromeTestWindow inherits Window {
        title: "Harmony isolated window chrome test";
        no-frame: true;
        preferred-width: 1120px; preferred-height: 760px;
        min-width: 800px; min-height: 560px;
        in-out property <bool> chrome: true;
        in-out property <bool> test-maximized;
        out property <length> observed-width: root.width;
        out property <length> observed-height: root.height;
        callback move-window;
        callback resize-window(int);
        callback minimize-window;
        callback maximize-window;
        callback close-window;
        if root.chrome && ClientStyle.enabled: WindowBar {
            x: 0px; y: 0px; width: root.width; focused: true; maximized: root.test-maximized;
            place: "Harmony";
            move-window => { root.move-window(); }
            minimize-window => { root.minimize-window(); }
            maximize-window => { root.maximize-window(); }
            close-window => { root.close-window(); }
        }
        if root.chrome && !ClientStyle.enabled: TitleBar {
            x: 0px; y: 0px; width: root.width; focused: true; maximized: root.test-maximized;
            place: "Harmony";
            move-window => { root.move-window(); }
            minimize-window => { root.minimize-window(); }
            maximize-window => { root.maximize-window(); }
            close-window => { root.close-window(); }
        }
        // The same overlay instantiation as YapfastWindow. Its default geometry
        // is deliberately exercised, as well as its production TouchAreas.
        if root.chrome && !root.test-maximized: ResizeEdges {
            resize-window(direction) => { root.resize-window(direction); }
        }
    }
}
