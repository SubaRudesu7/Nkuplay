// 视频画面上的鼠标操作，WinUI 3 原生实现（`NativeView`，声明在 lib.rs 的 `player` 里）。
//
// 一层透明的面盖在视频上：单击暂停或继续，双击切换全屏。Bern 的指针事件里有双击、没有单击，
// 两个都要时还得分清“单击”和“双击的第一下”，所以自己写：单击后等一小会儿，没等到第二下
// 才算单击。等的时间取系统双击间隔，但不超过 300 毫秒，免得暂停反应太慢；第二下来得比这更晚
// 但仍在系统双击间隔内时，WinUI 仍报双击，这时把刚才的暂停或继续撤回，再切全屏。
// 右键菜单由 Bern 挂在这一层上（lib.rs 里这个 `NativeView` 的 `ContextMenu`）。
//
// 和 seek_bar.winui3.cpp 一样，这个文件会被包含两次，只用 `xaml::` 等别名。

/// 双击判断要记住的东西。
struct GestureState {
    /// 刚才的单击已经切换过暂停，双击时要撤回。
    bool toggled = false;
};

inline xaml::FrameworkElement build(Props const& props, Actions const& actions, Theme const&) {
    using winrt::Windows::UI::Color;
    controls::Grid surface;
    // 透明的背景，空白处也能接到鼠标；没有背景的面在 XAML 里不接收点击。
    surface.Background(media::SolidColorBrush(Color{0, 0, 0, 0}));
    surface.HorizontalAlignment(xaml::HorizontalAlignment::Stretch);
    surface.VerticalAlignment(xaml::VerticalAlignment::Stretch);
    xaml::Automation::AutomationProperties::SetName(surface, props.label());

    auto state = std::make_shared<GestureState>();
    auto toggle = [props, actions] {
        if (props.playing())
            actions.tap_pause();
        else
            actions.tap_play();
    };

    xaml::DispatcherTimer wait;
    auto interval = (std::min)(GetDoubleClickTime(), 300u);
    wait.Interval(std::chrono::milliseconds(interval));
    // 计时器只弱引用自己，免得处理函数里的引用让它释放不了。
    wait.Tick([state, toggle](IInspectable const& sender, auto&&) {
        sender.as<xaml::DispatcherTimer>().Stop();
        toggle();
        state->toggled = true;
    });

    surface.Tapped([state, weakWait = winrt::make_weak(wait)](auto&&, xaml::Input::TappedRoutedEventArgs const& args) {
        auto wait = weakWait.get();
        if (!wait) return;
        state->toggled = false;
        wait.Stop();
        wait.Start();
        args.Handled(true);
    });
    surface.DoubleTapped([state, toggle, actions, weakWait = winrt::make_weak(wait)](auto&&, xaml::Input::DoubleTappedRoutedEventArgs const& args) {
        auto wait = weakWait.get();
        if (wait && wait.IsEnabled())
            wait.Stop();
        else if (state->toggled)
            toggle();
        state->toggled = false;
        args.Handled(true);
        actions.tap_fullscreen();
    });
    // 视频页拆掉时停下计时器，也让计时器活到那时。
    Reactive::OnCleanup([wait] { wait.Stop(); });

    return surface;
}
