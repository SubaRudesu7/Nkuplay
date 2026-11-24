// 视频页的进度条，WinUI 3 原生实现（`NativeView`，声明在 lib.rs 的 `play_bar` 里）。
//
// Bern 的 `Slider` 只能告诉界面鼠标在第几分之几处，`ui!` 又不能做算术、不能按指针位置摆东西，
// 所以进度条自己画：按章节分成几段的轨道、已播放的部分、轨道上方的弹幕热度曲线、
// 鼠标所在处的圆点，以及跟着指针走的时间气泡。章节和热度由引擎给（`chapters`、`heat`，可以为空）。
// 点一下或拖动后松开，就通过 `seek` 这一项把比例（0 到 1）交给引擎，由引擎换算成毫秒去跳。
//
// 这个文件被 Bern 生成的 `app_ui.inl` 包含两次（WinUI 3 和 XAML Islands 两种前端），
// 所以只用 `xaml::`、`controls::`、`media::` 这些别名，不直接写 `Microsoft::UI::Xaml`。
// `Props`、`Actions`、`Theme` 是 Bern 按声明生成的；`Reactive::Run` 是 Bern 运行时的响应式效果：
// 里面读到的值变了就重跑，视图被拆掉时自动停止。

/// 秒数写成 `m:ss`；`hours` 为真时写成 `h:mm:ss`。
inline winrt::hstring SeekClock(double seconds, bool hours) {
    if (!std::isfinite(seconds) || seconds < 0) seconds = 0;
    auto total = static_cast<long long>(std::floor(seconds));
    wchar_t buffer[32];
    if (hours)
        swprintf_s(buffer, L"%lld:%02lld:%02lld", total / 3600, total / 60 % 60, total % 60);
    else
        swprintf_s(buffer, L"%lld:%02lld", total / 60, total % 60);
    return buffer;
}

/// 四角都是 `radius` 的圆角。
inline xaml::CornerRadius Round(double radius) {
    return xaml::CornerRadius{radius, radius, radius, radius};
}

/// 进度条在鼠标操作之间要记住的东西。
struct SeekBarState {
    /// 鼠标在轨道上方。
    bool hovering = false;
    /// 按住鼠标在拖。
    bool dragging = false;
    /// 鼠标所在处，0 到 1。
    double pointer = 0;
    /// 松开后到播放器报回新位置之前，先显示在松开的地方，免得圆点跳回去再跳过来。
    std::optional<double> pending;
    ULONGLONG pendingSince = 0;
};

inline xaml::FrameworkElement build(Props const& props, Actions const& actions, Theme const& theme) {
    using winrt::Windows::UI::Color;
    // 时间气泡的高度、气泡和轨道的间距、能点的区域的高度、轨道的粗细、圆点的直径。
    // 气泡文字左右的内边距。
    constexpr double kBubble = 24, kGap = 2, kHit = 20, kTrack = 4, kThumb = 12, kPadX = 6;
    // 热度曲线的高度、章节之间的空隙。
    constexpr double kHeat = 30, kChapterGap = 3;
    auto white = [](uint8_t alpha) { return media::SolidColorBrush(Color{alpha, 255, 255, 255}); };

    // 最外层不接收鼠标（没有背景），只有下面那条能点的区域接收。
    controls::Grid root;
    root.Height(kBubble + kGap + kHit + kHeat - kHit / 2);
    root.HorizontalAlignment(xaml::HorizontalAlignment::Stretch);

    controls::Grid hit;
    hit.Height(kHit);
    hit.VerticalAlignment(xaml::VerticalAlignment::Bottom);
    hit.Background(media::SolidColorBrush(Color{0, 0, 0, 0}));
    xaml::Automation::AutomationProperties::SetName(hit, props.seek_label());

    // 轨道：每一章一段，段与段之间空开一点。底色、鼠标悬停时到鼠标处稍亮的部分、已播放的部分，
    // 每次摆放时重画。
    controls::Canvas track;
    track.Height(kTrack);
    track.VerticalAlignment(xaml::VerticalAlignment::Center);
    track.IsHitTestVisible(false);
    auto accentColor = theme.accent();
    auto accent = media::SolidColorBrush(accentColor);

    // 弹幕热度：轨道上方的一片曲线，播过的部分是强调色。
    xaml::Shapes::Path heat;
    heat.Fill(white(41));
    heat.Height(kHeat);
    heat.VerticalAlignment(xaml::VerticalAlignment::Bottom);
    heat.Margin(xaml::Thickness{0, 0, 0, kHit / 2});
    heat.IsHitTestVisible(false);
    xaml::Shapes::Path heatPlayed;
    heatPlayed.Fill(media::SolidColorBrush(Color{90, accentColor.R, accentColor.G, accentColor.B}));
    heatPlayed.Height(kHeat);
    heatPlayed.VerticalAlignment(xaml::VerticalAlignment::Bottom);
    heatPlayed.Margin(xaml::Thickness{0, 0, 0, kHit / 2});
    heatPlayed.IsHitTestVisible(false);
    media::RectangleGeometry heatClip;
    heatPlayed.Clip(heatClip);

    controls::Border thumb;
    thumb.Width(kThumb);
    thumb.Height(kThumb);
    thumb.CornerRadius(Round(kThumb / 2));
    thumb.VerticalAlignment(xaml::VerticalAlignment::Center);
    thumb.HorizontalAlignment(xaml::HorizontalAlignment::Left);
    thumb.Background(white(255));
    thumb.IsHitTestVisible(false);

    hit.Children().Append(track);
    hit.Children().Append(thumb);

    // 时间气泡：深色底、白字，摆在鼠标正上方，碰到两头就贴边。
    controls::TextBlock label;
    label.FontSize(12);
    label.Foreground(white(255));
    controls::Border bubble;
    bubble.Child(label);
    bubble.Padding(xaml::Thickness{kPadX, 2, kPadX, 2});
    bubble.CornerRadius(Round(4));
    bubble.Background(media::SolidColorBrush(Color{230, 24, 24, 24}));
    bubble.VerticalAlignment(xaml::VerticalAlignment::Top);
    bubble.HorizontalAlignment(xaml::HorizontalAlignment::Left);
    bubble.IsHitTestVisible(false);

    root.Children().Append(heat);
    root.Children().Append(heatPlayed);
    root.Children().Append(hit);
    root.Children().Append(bubble);

    auto state = std::make_shared<SeekBarState>();
    // 热度曲线按宽度和数据画；两样都没变就不重画。
    auto heatFor = std::make_shared<std::pair<double, std::vector<double>>>();
    // 下面的处理函数挂在 `hit` 上；它们只弱引用 `hit`，免得互相引用、拆掉后释放不了。
    auto weakHit = winrt::make_weak(hit);

    // 按当前状态摆放各部分。播放器的进度变化（效果重跑）、尺寸变化、鼠标移动时都调用它。
    auto layout = [=] {
        auto hit = weakHit.get();
        if (!hit) return;
        double width = hit.ActualWidth();
        double progress = (std::clamp)(props.seek(), 0.0, 1.0);
        double duration = props.duration();
        if (state->pending) {
            // 播放器报回的位置到了松开的地方附近，或者等得太久，就不再替它显示。
            bool arrived = std::abs(progress - *state->pending) < 0.002;
            bool stale = GetTickCount64() - state->pendingSince > 1500;
            if (arrived || stale) state->pending.reset();
        }
        double shown = state->dragging ? state->pointer : state->pending.value_or(progress);
        bool active = state->hovering || state->dragging;

        // 各章的起点（不含 0），排好、去掉范围外的。
        std::vector<double> cuts;
        for (double cut : props.chapters())
            if (cut > 0 && cut < 1) cuts.push_back(cut);
        std::sort(cuts.begin(), cuts.end());
        cuts.insert(cuts.begin(), 0.0);
        cuts.push_back(1.0);
        track.Children().Clear();
        auto bar = [&](double from, double to, media::Brush const& brush) {
            if (to - from < 0.5) return;
            controls::Border piece;
            piece.Width(to - from);
            piece.Height(kTrack);
            piece.CornerRadius(Round(kTrack / 2));
            piece.Background(brush);
            controls::Canvas::SetLeft(piece, from);
            track.Children().Append(piece);
        };
        for (size_t i = 0; i + 1 < cuts.size(); ++i) {
            double from = width * cuts[i] + (i > 0 ? kChapterGap / 2 : 0);
            double to = width * cuts[i + 1] - (i + 2 < cuts.size() ? kChapterGap / 2 : 0);
            bar(from, to, white(70));
            // 鼠标悬停时，从开头到鼠标处稍亮一点，看得出要跳到哪里。
            if (state->hovering) bar(from, (std::min)(to, width * state->pointer), white(60));
            bar(from, (std::min)(to, width * shown), accent);
        }
        auto values = props.heat();
        if (heatFor->first != width || heatFor->second != values) {
            heatFor->first = width;
            heatFor->second = values;
            // 同一份几何不能给两个元素用，强调色的那份另做一份。
            auto shape = [&]() {
                media::PathGeometry geometry;
                if (values.size() < 2 || width <= 0) return geometry;
                media::PathFigure figure;
                figure.StartPoint({0, static_cast<float>(kHeat)});
                figure.IsClosed(true);
                auto to = [&figure](double x, double y) {
                    media::LineSegment line;
                    line.Point({static_cast<float>(x), static_cast<float>(y)});
                    figure.Segments().Append(line);
                };
                auto level = [&](size_t i) { return kHeat - (std::clamp)(values[i], 0.0, 1.0) * (kHeat - 2); };
                to(0, level(0));
                for (size_t i = 0; i < values.size(); ++i) to((i + 0.5) / values.size() * width, level(i));
                to(width, level(values.size() - 1));
                to(width, kHeat);
                geometry.Figures().Append(figure);
                return geometry;
            };
            heat.Data(shape());
            heatPlayed.Data(shape());
        }
        heatClip.Rect({0, 0, static_cast<float>(width * shown), static_cast<float>(kHeat)});
        thumb.Visibility(active ? xaml::Visibility::Visible : xaml::Visibility::Collapsed);
        thumb.Margin(xaml::Thickness{width * shown - kThumb / 2, 0, 0, 0});

        if (!active || duration <= 0 || width <= 0) {
            bubble.Visibility(xaml::Visibility::Collapsed);
            return;
        }
        label.Text(SeekClock(state->pointer * duration, duration >= 3600));
        bubble.Visibility(xaml::Visibility::Visible);
        // 量文字而不是量气泡：XAML 的 DesiredSize 含元素自己的 Margin，气泡的 Margin 正是上一次
        // 摆的位置，量气泡会把它算进宽度，指针越靠右气泡越往左偏。文字没有 Margin，加上左右内边距即可。
        label.Measure(Size{std::numeric_limits<float>::infinity(), std::numeric_limits<float>::infinity()});
        double bubbleWidth = label.DesiredSize().Width + kPadX * 2;
        double left = (std::clamp)(width * state->pointer - bubbleWidth / 2, 0.0, (std::max)(0.0, width - bubbleWidth));
        bubble.Margin(xaml::Thickness{left, 0, 0, 0});
    };

    // 读进度和总时长的地方放在效果里，它们一变就重新摆放。
    Reactive::Run([props, layout] {
        props.seek();
        props.duration();
        props.chapters();
        props.heat();
        Reactive::Untrack([&] { layout(); });
    });

    // 鼠标在轨道的第几分之几处。
    auto at = [](IInspectable const& sender, xaml::Input::PointerRoutedEventArgs const& args) {
        auto hit = sender.as<xaml::UIElement>();
        double width = hit.ActualSize().x;
        if (width <= 0) return 0.0;
        return (std::clamp)(args.GetCurrentPoint(hit).Position().X / width, 0.0, 1.0);
    };

    hit.SizeChanged([layout](auto&&, auto&&) { layout(); });
    hit.PointerEntered([state, layout, at](IInspectable const& sender, xaml::Input::PointerRoutedEventArgs const& args) {
        state->hovering = true;
        state->pointer = at(sender, args);
        layout();
    });
    hit.PointerMoved([state, layout, at](IInspectable const& sender, xaml::Input::PointerRoutedEventArgs const& args) {
        state->pointer = at(sender, args);
        layout();
    });
    hit.PointerExited([state, layout](auto&&, auto&&) {
        state->hovering = false;
        layout();
    });
    hit.PointerPressed([state, layout, at](IInspectable const& sender, xaml::Input::PointerRoutedEventArgs const& args) {
        auto hit = sender.as<xaml::UIElement>();
        if (!args.GetCurrentPoint(hit).Properties().IsLeftButtonPressed()) return;
        state->dragging = true;
        state->pointer = at(sender, args);
        hit.CapturePointer(args.Pointer());
        args.Handled(true);
        layout();
    });
    hit.PointerReleased([state, layout, at, actions](IInspectable const& sender, xaml::Input::PointerRoutedEventArgs const& args) {
        auto hit = sender.as<xaml::UIElement>();
        if (!state->dragging) return;
        state->dragging = false;
        state->pointer = at(sender, args);
        state->pending = state->pointer;
        state->pendingSince = GetTickCount64();
        hit.ReleasePointerCapture(args.Pointer());
        args.Handled(true);
        actions.set_seek(state->pointer);
        layout();
    });
    // 拖的时候被别的东西抢走了鼠标（比如切到别的窗口）：不跳。
    hit.PointerCaptureLost([state, layout](auto&&, auto&&) {
        if (!state->dragging) return;
        state->dragging = false;
        layout();
    });

    return root;
}
