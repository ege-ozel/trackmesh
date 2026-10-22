#include "view.h"
#include "plugin.h"
#include <algorithm>
#include <cstdio>
#include <cstring>
#include <ctime>
#include <utility>
#include <uxtheme.h>

using namespace Steinberg;
namespace TrackMesh {
struct Tone { COLORREF fill, edge, ink; };
namespace {
constexpr int width = 580, height = 384;
const ViewRect rectangle(0, 0, width, height);
// The TrackMesh window's tokens (apps/desktop/src/styles.css), so the plugin reads as the same project card.
constexpr COLORREF surface = RGB(18, 29, 44), paneFill = RGB(20, 31, 47), inputFill = RGB(13, 22, 35);
constexpr COLORREF paneBorder = RGB(37, 48, 68), divider = RGB(39, 52, 73);
constexpr COLORREF buttonFill = RGB(26, 37, 53), buttonHover = RGB(40, 56, 79), buttonBorder = RGB(48, 60, 79), inputBorder = RGB(52, 66, 87);
constexpr COLORREF text = RGB(233, 237, 245), text2 = RGB(178, 194, 214), muted = RGB(139, 156, 179), link = RGB(170, 200, 235);
constexpr COLORREF accent = RGB(130, 223, 181), accentHover = RGB(155, 236, 206), accentInk = RGB(16, 37, 29), accentEdge = RGB(228, 255, 242), accentShade = RGB(79, 174, 134);
constexpr COLORREF warn = RGB(220, 186, 127), danger = RGB(243, 168, 157), badgeText = RGB(156, 170, 189), badgeBorder = RGB(60, 72, 90);
constexpr Tone idleTone {surface, badgeBorder, badgeText}, onlineTone {RGB(21, 55, 43), RGB(55, 100, 84), accent},
    warnTone {RGB(45, 41, 27), RGB(170, 141, 69), warn}, dangerTone {RGB(53, 34, 41), RGB(113, 64, 74), danger},
    infoTone {RGB(22, 39, 60), RGB(52, 85, 125), link};
// Content column, in 96-DPI pixels; the whole window is the card.
constexpr int left = 32, right = 548, paneSplit = 290;
enum Control { Share = 1, Get = 2, Picker = 10, Choose = 11, Change = 12, Open = 13, AutoShare = 14 };
std::mutex classMutex;
size_t classViews = 0;
// A disabled control is drawn at 40 % over its backdrop, like the app's `opacity:.4`.
COLORREF mix(COLORREF value, COLORREF backdrop, int percent) {
    const auto part = [&](int a, int b) { return (a * percent + b * (100 - percent)) / 100; };
    return RGB(part(GetRValue(value), GetRValue(backdrop)), part(GetGValue(value), GetGValue(backdrop)), part(GetBValue(value), GetBValue(backdrop)));
}
void fill(HDC dc, RECT area, COLORREF color) {
    const auto brush = CreateSolidBrush(color);
    FillRect(dc, &area, brush);
    DeleteObject(brush);
}
void rounded(HDC dc, RECT area, int radius, COLORREF inside, COLORREF edge, int thickness = 1) {
    const auto brush = inside == CLR_INVALID ? static_cast<HBRUSH>(GetStockObject(NULL_BRUSH)) : CreateSolidBrush(inside);
    const auto pen = CreatePen(PS_INSIDEFRAME, thickness, edge);
    const auto oldBrush = SelectObject(dc, brush), oldPen = SelectObject(dc, pen);
    RoundRect(dc, area.left, area.top, area.right, area.bottom, radius * 2, radius * 2);
    SelectObject(dc, oldBrush);
    SelectObject(dc, oldPen);
    DeleteObject(pen);
    if (inside != CLR_INVALID) DeleteObject(brush);
}
void label(HDC dc, HFONT font, const std::wstring& value, RECT area, COLORREF color, UINT flags = DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS | DT_VCENTER) {
    const auto previous = SelectObject(dc, font);
    SetTextColor(dc, color);
    DrawTextW(dc, value.c_str(), static_cast<int>(value.size()), &area, flags | DT_NOPREFIX);
    SelectObject(dc, previous);
}
// Wrapped text centred vertically in its area; the last visible line ends in an ellipsis.
void paragraph(HDC dc, HFONT font, const std::wstring& value, RECT area, COLORREF color) {
    const auto previous = SelectObject(dc, font);
    RECT measured = area;
    DrawTextW(dc, value.c_str(), static_cast<int>(value.size()), &measured, DT_LEFT | DT_WORDBREAK | DT_NOPREFIX | DT_CALCRECT);
    const auto spare = (area.bottom - area.top) - (measured.bottom - measured.top);
    if (spare > 0) area.top += spare / 2;
    SetTextColor(dc, color);
    DrawTextW(dc, value.c_str(), static_cast<int>(value.size()), &area, DT_LEFT | DT_WORDBREAK | DT_NOPREFIX | DT_END_ELLIPSIS | DT_EDITCONTROL);
    SelectObject(dc, previous);
}
int textWidth(HDC dc, HFONT font, const std::wstring& value) {
    const auto previous = SelectObject(dc, font);
    SIZE size {};
    GetTextExtentPoint32W(dc, value.c_str(), static_cast<int>(value.size()), &size);
    SelectObject(dc, previous);
    return size.cx;
}
// The plugin follows the TrackMesh window's language; older agents send none, which reads as English.
std::wstring pick(const ViewStatus& status, const wchar_t* english, const wchar_t* turkish) {
    return status.language == L"tr" ? turkish : english;
}
void setText(HWND control, const std::wstring& value) {
    wchar_t current[128] {};
    GetWindowTextW(control, current, 128);
    if (value != current) { SetWindowTextW(control, value.c_str()); InvalidateRect(control, nullptr, FALSE); }
}
// RFC 3339 UTC from the agent ("2026-10-14T12:34:56+00:00") as local "14.10.2026 15:34", as in the app; empty if unreadable.
std::wstring localTime(const std::wstring& value) {
    tm utc {};
    if (swscanf_s(value.c_str(), L"%4d-%2d-%2dT%2d:%2d", &utc.tm_year, &utc.tm_mon, &utc.tm_mday, &utc.tm_hour, &utc.tm_min) != 5) return {};
    utc.tm_year -= 1900;
    utc.tm_mon -= 1;
    const auto seconds = _mkgmtime64(&utc);
    tm local {};
    if (seconds == -1 || _localtime64_s(&local, &seconds) != 0) return {};
    wchar_t buffer[32] {};
    return wcsftime(buffer, 32, L"%d.%m.%Y %H:%M", &local) ? std::wstring(buffer) : std::wstring();
}
}
View::View(std::shared_ptr<Bridge> b, Plugin* p) : CPluginView(&rectangle), bridge(std::move(b)), plugin(p) {
    plugin->addRef();
    makeFonts();
    inputBrush = CreateSolidBrush(inputFill);
}
// The app's type scale: card title 20, body and buttons 14, meta 13, badge 12.
void View::makeFonts() {
    for (auto item : {font, titleFont, smallFont, strongFont, badgeFont}) if (item) DeleteObject(item);
    const auto make = [&](int size, int weight) {
        return CreateFontW(-px(size), 0, 0, 0, weight, FALSE, FALSE, FALSE, DEFAULT_CHARSET, 0, 0, CLEARTYPE_QUALITY, 0, L"Segoe UI");
    };
    font = make(14, FW_NORMAL);
    titleFont = make(20, FW_SEMIBOLD);
    smallFont = make(13, FW_NORMAL);
    strongFont = make(14, FW_SEMIBOLD);
    badgeFont = make(12, FW_SEMIBOLD);
}
View::~View() {
    removed();
    for (auto item : {font, titleFont, smallFont, strongFont, badgeFont}) if (item) DeleteObject(item);
    if (inputBrush) DeleteObject(inputBrush);
    plugin->release();
}
tresult PLUGIN_API View::isPlatformTypeSupported(FIDString type) { return type && std::strcmp(type, kPlatformTypeHWND) == 0 ? kResultTrue : kResultFalse; }
tresult PLUGIN_API View::attached(void* parent, FIDString type) {
    if (!parent || window || isPlatformTypeSupported(type) != kResultTrue) return kResultFalse;
    HMODULE module = nullptr;
    GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        reinterpret_cast<LPCWSTR>(&procedure), &module);
    WNDCLASSW klass {};
    klass.lpfnWndProc = procedure;
    klass.hInstance = module;
    klass.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    klass.lpszClassName = L"TrackMeshVST3View-v1";
    {
        std::lock_guard<std::mutex> lock(classMutex);
        if (!RegisterClassW(&klass) && GetLastError() != ERROR_CLASS_ALREADY_EXISTS) return kResultFalse;
        ++classViews;
        registeredClass = true;
        viewModule = module;
    }
    // A DPI-aware host gets a view scaled to its monitor; an unaware one reports 96 and Windows scales it.
    const auto hostDpi = static_cast<int>(GetDpiForWindow(static_cast<HWND>(parent)));
    if (hostDpi > 0 && hostDpi != dpi) { dpi = hostDpi; makeFonts(); }
    window = CreateWindowExW(0, klass.lpszClassName, L"TrackMesh", WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN,
        0, 0, px(width), px(height), static_cast<HWND>(parent), nullptr, module, this);
    if (!window) { removed(); return kResultFalse; }
    const auto make = [&](const wchar_t* cls, DWORD style, int x, int y, int w, int h, int id, HFONT controlFont) {
        const auto control = CreateWindowExW(0, cls, L"", WS_CHILD | WS_VISIBLE | style, px(x), px(y), px(w), px(h), window,
            reinterpret_cast<HMENU>(static_cast<INT_PTR>(id)), module, nullptr);
        SendMessageW(control, WM_SETFONT, reinterpret_cast<WPARAM>(controlFont), TRUE);
        return control;
    };
    // The projects show as an owner-drawn list rather than a drop-down: nothing to open first, and every
    // part of it uses the card's colours (a closed combo box keeps Windows' light border and arrow).
    picker = make(L"LISTBOX", WS_TABSTOP | WS_VSCROLL | LBS_OWNERDRAWFIXED | LBS_HASSTRINGS | LBS_NOTIFY | LBS_NOINTEGRALHEIGHT,
        left + 2, 106, 338, 32, Picker, font);
    SetWindowTheme(picker, L"DarkMode_Explorer", nullptr); // dark scroll bar on Windows 10 1809 and later
    SendMessageW(picker, LB_SETITEMHEIGHT, 0, px(36)); // rows match the 36 px buttons
    chooseButton = make(L"BUTTON", WS_TABSTOP | BS_OWNERDRAW, 386, 104, right - 386, 36, Choose, strongFont); // wide enough for "Bu projeyi kullan"
    // Share and Get sit at the bottom of their panes, as in the app's card.
    sendButton = make(L"BUTTON", WS_TABSTOP | BS_OWNERDRAW, left + 16, 190, paneSplit - 8 - left - 32, 36, Share, strongFont);
    receiveButton = make(L"BUTTON", WS_TABSTOP | BS_OWNERDRAW, paneSplit + 8 + 16, 190, right - paneSplit - 8 - 32, 36, Get, strongFont);
    autoShareCheckbox = make(L"BUTTON", WS_TABSTOP | BS_OWNERDRAW, left, 250, right - left, 24, AutoShare, font);
    // Footer text buttons; refresh() sizes them to their words.
    changeButton = make(L"BUTTON", WS_TABSTOP | BS_OWNERDRAW, left, 344, 120, 26, Change, strongFont);
    openButton = make(L"BUTTON", WS_TABSTOP | BS_OWNERDRAW, left, 344, 120, 26, Open, strongFont);
    refresh();
    SetTimer(window, 1, 250, nullptr);
    CPluginView::attached(parent, type);
    if (dpi != 96) {
        ViewRect scaled(0, 0, px(width), px(height));
        setRect(scaled);
        if (plugFrame) plugFrame->resizeView(this, &scaled);
    }
    return kResultOk;
}
tresult PLUGIN_API View::removed() {
    if (window) { KillTimer(window, 1); DestroyWindow(window); window = nullptr; }
    if (registeredClass) {
        std::lock_guard<std::mutex> lock(classMutex);
        if (classViews > 0 && --classViews == 0) UnregisterClassW(L"TrackMeshVST3View-v1", viewModule);
        registeredClass = false;
    }
    sendButton = receiveButton = picker = chooseButton = changeButton = openButton = nullptr;
    autoShareCheckbox = nullptr;
    return CPluginView::removed();
}
// A framed notice like the app's `.notice`/`.problem`: tinted fill, coloured edge, wrapped text.
void View::notice(HDC dc, RECT area, const Tone& tone, const std::wstring& value, HFONT textFont, COLORREF ink) const {
    rounded(dc, area, px(8), tone.fill, tone.edge);
    paragraph(dc, textFont, value, {area.left + px(14), area.top + px(4), area.right - px(14), area.bottom - px(4)}, ink);
}
void View::paint(HWND handle) {
    PAINTSTRUCT paintInfo {};
    const auto target = BeginPaint(handle, &paintInfo);
    RECT client {};
    GetClientRect(handle, &client);
    // Drawn off-screen and copied once, so the 250 ms refresh never flickers.
    const auto dc = CreateCompatibleDC(target);
    const auto bitmap = CreateCompatibleBitmap(target, client.right, client.bottom);
    const auto oldBitmap = SelectObject(dc, bitmap);
    fill(dc, client, surface);
    SetBkMode(dc, TRANSPARENT);
    const auto status = bridge->status();
    const bool selecting = !status.linked || changing;


    // Title row: project name and the same connection badge as the Desktop card.
    const bool away = status.state == L"AGENT UNAVAILABLE";
    const auto collaborator = status.author;
    // The badge names everyone this project is shared with; sentences below name the one Get uses.
    const auto everyone = status.collaborators.empty() ? collaborator : status.collaborators;
    std::wstring badge;
    const Tone* tone = &idleTone;
    if (!status.connected) { badge = away ? pick(status, L"TrackMesh not reachable", L"TrackMesh'e ulaşılamıyor") : pick(status, L"Connecting…", L"Bağlanıyor…"); tone = away ? &warnTone : &idleTone; }
    else if (selecting) badge.clear();
    else if (status.peerOnline) { badge = pick(status, L"Connected", L"Bağlı") + (everyone.empty() ? L"" : L" · " + everyone); tone = &onlineTone; }
    else if (everyone.empty()) badge = pick(status, L"No collaborator yet", L"Henüz iş arkadaşı yok");
    else { badge = pick(status, L"Not connected · ", L"Bağlı değil · ") + everyone; tone = &warnTone; }
    int titleRight = px(right);
    if (!badge.empty()) {
        const auto badgeWidth = std::min(textWidth(dc, badgeFont, badge) + px(26), px(260));
        RECT pill {px(right) - badgeWidth, px(34), px(right), px(58)};
        rounded(dc, pill, (pill.bottom - pill.top) / 2, tone->fill, tone->edge);
        label(dc, badgeFont, badge, {pill.left + px(12), pill.top, pill.right - px(12), pill.bottom}, tone->ink, DT_CENTER | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
        titleRight = pill.left - px(12);
    }
    label(dc, titleFont, selecting ? pick(status, L"Which TrackMesh project is this?", L"Bu hangi TrackMesh projesi?") : status.name,
        {px(left), px(28), titleRight, px(64)}, text);

    if (!selecting) {
        // One status line: a warning (also for split versions and an unfinished Get), the collaborator's new version in blue, or the state with that version's time.
        const auto when = localTime(status.createdAt);
        const auto stamp = collaborator.empty() || when.empty() ? std::wstring() : L" · " + when;
        if (confirming) notice(dc, box(left, 72, right, 118), warnTone, status.shareConfirmation, font, text);
        else if (!status.presenceWarning.empty())
            notice(dc, box(left, 72, right, 118), status.presenceDanger ? dangerTone : warnTone, status.presenceWarning, smallFont, text);
        else if (status.state == L"DIVERGED" || status.state == L"RECOVERY_REQUIRED")
            notice(dc, box(left, 72, right, 118), warnTone, status.line, smallFont, text);
        else if (status.state == L"REMOTE_AHEAD") notice(dc, box(left, 76, right, 112), infoTone, status.line + stamp, strongFont, link);
        else if (status.state == L"UP_TO_DATE") paragraph(dc, font, status.line + stamp, box(left, 72, right, 118), text2);
        else {
            paragraph(dc, font, status.line, box(left, 70, right, 96), text2);
            if (!stamp.empty()) label(dc, smallFont, pick(status, L"Latest version from ", L"Son sürüm: ") + collaborator + stamp, box(left, 96, right, 118), muted);
        }
        // Two panes, each with one line on what its button does; none while Share waits for a second press.
        const auto name = collaborator.empty() ? pick(status, L"your collaborator", L"iş arkadaşın") : collaborator;
        const RECT sharePane = box(left, 128, paneSplit - 8, 240), getPane = box(paneSplit + 8, 128, right, 240);
        rounded(dc, sharePane, px(8), paneFill, paneBorder);
        rounded(dc, getPane, px(8), paneFill, paneBorder);
        paragraph(dc, smallFont, pick(status, L"Saves this folder as a new version ", L"Bu klasörü ") + name + pick(status, L" can get.", L" için yeni bir sürüm olarak kaydeder."),
            {sharePane.left + px(16), sharePane.top + px(10), sharePane.right - px(16), sharePane.top + px(46)}, muted);
        const bool nothingNew = status.state == L"UP_TO_DATE" || status.state == L"LOCAL_AHEAD";
        // A disabled Get explains nothing here; the status line above says why.
        const auto getHint = confirming || (!status.getEnabled && !nothingNew) ? std::wstring()
            : nothingNew ? pick(status, L"Nothing new to get.", L"Alınacak yeni sürüm yok.")
            : pick(status, L"Opens TrackMesh on this project. Close the project in your DAW first.", L"TrackMesh'i bu projede açar. Önce projeyi DAW'da kapat.");
        paragraph(dc, smallFont, getHint, {getPane.left + px(16), getPane.top + px(10), getPane.right - px(16), getPane.top + px(46)}, muted);
    } else {
        const auto hint = !status.pickerSupported ? (status.connected ? status.line : status.message)
            : choices.empty() ? pick(status, L"Add this project's folder in TrackMesh first (Add project).", L"Önce bu projenin klasörünü TrackMesh'e ekle (Proje ekle).")
            : pick(status, L"Choose the TrackMesh project whose folder holds this DAW project.", L"Bu DAW projesinin klasörünü içeren TrackMesh projesini seç.");
        paragraph(dc, font, hint, box(left, 68, right, 96), text2);
        // The list's frame, drawn around the child list box like the app's input border.
        if (status.pickerSupported && !choices.empty()) {
            auto frame = pickerArea();
            InflateRect(&frame, px(2), px(2));
            rounded(dc, frame, px(6), inputFill, inputBorder);
        }
        if (status.projectsMore) paragraph(dc, smallFont, pick(status, L"Showing the first 64 TrackMesh projects.", L"TrackMesh'teki ilk 64 proje gösteriliyor."), box(386, 152, right, 198), muted);
    }

    // A refusal in a red notice, an unreachable agent in amber, anything else as a quiet line; never the status line twice.
    // A host that can't mark the project changed keeps the choice only after its next save.
    const bool unsaved = !selecting && selectionError.empty() && plugin->saveNeeded();
    const auto message = !selectionError.empty() ? selectionError
        : unsaved ? pick(status, L"Save the project in your DAW to keep this project choice.", L"Bu proje seçimini korumak için projeyi DAW'da kaydet.")
        : status.message;
    // An unlinked plugin's agent reply only says "not linked"; the picker hint above already covers that.
    const bool repeated = selecting ? selectionError.empty() && !status.linked
        : !unsaved && (message == status.line || message == status.presenceWarning);
    if (!message.empty() && !repeated) {
        if (!selectionError.empty() || status.refused || status.state == L"ACTION REFUSED") notice(dc, box(left, 282, right, 330), dangerTone, message, smallFont, text);
        else if (away || unsaved) notice(dc, box(left, 282, right, 330), warnTone, message, smallFont, text);
        else paragraph(dc, smallFont, message, box(left, 282, right, 330), muted);
    }
    fill(dc, box(left, 338, right, 339), divider);

    BitBlt(target, 0, 0, client.right, client.bottom, dc, 0, 0, SRCCOPY);
    SelectObject(dc, oldBitmap);
    DeleteObject(bitmap);
    DeleteDC(dc);
    EndPaint(handle, &paintInfo);
}
void View::place(HWND control, RECT area) {
    RECT current {};
    GetWindowRect(control, &current);
    MapWindowPoints(nullptr, window, reinterpret_cast<POINT*>(&current), 2);
    if (!EqualRect(&current, &area))
        SetWindowPos(control, nullptr, area.left, area.top, area.right - area.left, area.bottom - area.top, SWP_NOZORDER | SWP_NOACTIVATE);
}
RECT View::pickerArea() const {
    const auto rows = static_cast<int>(std::clamp<size_t>(choices.size(), 1, 4));
    return box(left + 2, 106, 372, 106 + rows * 36);
}
void View::chooseSelected() {
    const auto index = SendMessageW(picker, LB_GETCURSEL, 0, 0);
    if (index < 0 || static_cast<size_t>(index) >= choices.size()) return;
    if (plugin->selectProject(choices[static_cast<size_t>(index)].id) != Plugin::Selection::Refused) {
        changing = false; selectionError.clear();
    } else selectionError = pick(bridge->status(), L"That project can't be used. Nothing was changed.", L"Bu proje kullanılamıyor. Hiçbir şey değişmedi.");
    refresh();
}
void View::refresh() {
    const auto status = bridge->status();
    bool same = choices.size() == status.projects.size();
    for (size_t i = 0; same && i < choices.size(); ++i)
        same = choices[i].id == status.projects[i].id && choices[i].name == status.projects[i].name && choices[i].folder == status.projects[i].folder;
    if (!same) {
        const auto index = SendMessageW(picker, LB_GETCURSEL, 0, 0);
        const auto selected = index >= 0 && static_cast<size_t>(index) < choices.size() ? choices[static_cast<size_t>(index)].id : ProjectId{};
        choices = status.projects;
        SendMessageW(picker, LB_RESETCONTENT, 0, 0);
        for (size_t i = 0; i < choices.size(); ++i) {
            auto entry = choices[i].name;
            if (std::count_if(choices.begin(), choices.end(), [&](const auto& choice) { return choice.name == entry; }) > 1)
                entry += L" (" + choices[i].folder + L")";
            SendMessageW(picker, LB_ADDSTRING, 0, reinterpret_cast<LPARAM>(entry.c_str()));
            if (choices[i].id == selected) SendMessageW(picker, LB_SETCURSEL, i, 0);
        }
        // With a single project there is nothing to choose between: select it, so Use project is one press.
        if (choices.size() == 1 && SendMessageW(picker, LB_GETCURSEL, 0, 0) == LB_ERR) SendMessageW(picker, LB_SETCURSEL, 0, 0);
        place(picker, pickerArea());
        InvalidateRect(window, nullptr, FALSE); // the list's frame follows its height
    }
    const bool selecting = !status.linked || changing;
    const bool alone = selecting && choices.empty();
    if (selecting || status.shareConfirmation.empty() || !status.shareEnabled) confirming = false;
    ShowWindow(picker, selecting && !choices.empty() ? SW_SHOW : SW_HIDE);
    ShowWindow(chooseButton, selecting && !choices.empty() ? SW_SHOW : SW_HIDE);
    ShowWindow(changeButton, status.linked ? SW_SHOW : SW_HIDE);
    setText(changeButton, changing ? pick(status, L"Cancel", L"Vazgeç") : pick(status, L"Change project", L"Projeyi değiştir"));
    setText(chooseButton, pick(status, L"Use project", L"Bu projeyi kullan"));
    // With no project to choose, Open TrackMesh is the one green button; otherwise a footer link like the app's "Advanced →".
    setText(openButton, alone ? pick(status, L"Open TrackMesh", L"TrackMesh'i aç") : pick(status, L"Open TrackMesh →", L"TrackMesh'i aç →"));
    setText(autoShareCheckbox, pick(status, L"Share when I save in the DAW", L"DAW'da kaydedince paylaş"));
    {
        const auto dc = GetDC(window);
        const auto measure = [&](HWND control) {
            wchar_t current[128] {};
            GetWindowTextW(control, current, 128);
            return textWidth(dc, strongFont, current) + px(8);
        };
        const auto openWidth = measure(openButton), changeWidth = measure(changeButton);
        ReleaseDC(window, dc);
        if (alone) place(openButton, box(left, 104, left + 200, 140));
        else place(openButton, {px(right) - openWidth, px(344), px(right), px(370)});
        const auto changeRight = alone ? px(right) : px(right) - openWidth - px(16);
        place(changeButton, {changeRight - changeWidth, px(344), changeRight, px(370)});
    }
    ShowWindow(sendButton, selecting ? SW_HIDE : SW_SHOW);
    ShowWindow(receiveButton, selecting ? SW_HIDE : SW_SHOW);
    ShowWindow(autoShareCheckbox, !selecting && status.autoShareSupported ? SW_SHOW : SW_HIDE);
    EnableWindow(autoShareCheckbox, status.connected && status.autoShareSupported);
    EnableWindow(sendButton, status.connected && status.shareEnabled);
    EnableWindow(receiveButton, confirming || (status.connected && status.getEnabled));
    EnableWindow(openButton, status.connected && status.desktopAvailable);
    EnableWindow(chooseButton, status.connected && SendMessageW(picker, LB_GETCURSEL, 0, 0) != LB_ERR);
    // Owner-drawn buttons repaint only when asked; ask when their words or colours change.
    const auto labels = status.language + L"|" + status.getLabel + L"|" + status.state + (status.autoShare ? L"|on" : L"|off")
        + (confirming ? L"|confirm" : L"") + (selecting ? L"|select" : L"") + (choices.empty() ? L"|none" : L"");
    if (labels != drawnLabels) {
        drawnLabels = labels;
        for (auto control : {sendButton, receiveButton, autoShareCheckbox, openButton, changeButton, chooseButton}) InvalidateRect(control, nullptr, FALSE);
    }
    InvalidateRect(window, nullptr, FALSE);
}
// Owner-drawn controls in the app's three button styles: green primary, dark secondary and text link.
void View::drawItem(const DRAWITEMSTRUCT& item) {
    const auto dc = item.hDC;
    const auto area = item.rcItem;
    if (item.CtlType == ODT_LISTBOX) {
        // A project row: the selected one in the green "Connected" tone with an accent edge, like a chosen option.
        fill(dc, area, inputFill);
        if (item.itemID == static_cast<UINT>(-1)) return;
        const bool chosen = (item.itemState & ODS_SELECTED) != 0;
        const RECT row {area.left + px(4), area.top + px(3), area.right - px(4), area.bottom - px(3)};
        if (chosen) rounded(dc, row, px(5), onlineTone.fill, onlineTone.edge);
        const auto length = SendMessageW(item.hwndItem, LB_GETTEXTLEN, item.itemID, 0);
        std::wstring name(length > 0 ? static_cast<size_t>(length) : 0, L'\0');
        if (length > 0) SendMessageW(item.hwndItem, LB_GETTEXT, item.itemID, reinterpret_cast<LPARAM>(name.data()));
        SetBkMode(dc, TRANSPARENT);
        label(dc, chosen ? strongFont : font, name, {row.left + px(12), row.top, row.right - px(12), row.bottom}, chosen ? accent : text);
        if ((item.itemState & ODS_FOCUS) && !(item.itemState & ODS_NOFOCUSRECT)) rounded(dc, row, px(5), CLR_INVALID, accent, px(2));
        return;
    }
    const bool disabled = (item.itemState & ODS_DISABLED) != 0, pressed = (item.itemState & ODS_SELECTED) != 0;
    const bool focused = (item.itemState & ODS_FOCUS) && !(item.itemState & ODS_NOFOCUSRECT); // keyboard focus only
    const auto status = bridge->status();
    const auto backdrop = item.CtlID == Share || item.CtlID == Get ? paneFill : surface;
    fill(dc, area, backdrop);
    SetBkMode(dc, TRANSPARENT);
    wchar_t current[128] {};
    GetWindowTextW(item.hwndItem, current, 128);
    if (item.CtlID == AutoShare) {
        // Share on save: the app's 18 px checkbox, filled with the accent and ticked when on.
        const auto top = area.top + (area.bottom - area.top - px(18)) / 2;
        const RECT square {area.left, top, area.left + px(18), top + px(18)};
        const bool on = status.autoShare;
        rounded(dc, square, px(4), mix(on ? accent : inputFill, backdrop, disabled ? 40 : 100), mix(on ? accent : inputBorder, backdrop, disabled ? 40 : 100));
        if (on) {
            const auto pen = CreatePen(PS_SOLID, px(2), accentInk);
            const auto oldPen = SelectObject(dc, pen);
            const POINT tick[] {{square.left + px(4), square.top + px(9)}, {square.left + px(7), square.top + px(12)}, {square.left + px(14), square.top + px(5)}};
            Polyline(dc, tick, 3);
            SelectObject(dc, oldPen);
            DeleteObject(pen);
        }
        label(dc, font, current, {square.right + px(10), area.top, area.right, area.bottom}, disabled ? muted : text);
        if (focused) rounded(dc, {square.left - px(2), square.top - px(2), square.right + px(2), square.bottom + px(2)}, px(5), CLR_INVALID, accent, px(2));
        return;
    }
    // Footer links: Change project, and Open TrackMesh unless it is the only action.
    const bool alone = (!status.linked || changing) && choices.empty();
    if (item.CtlID == Change || (item.CtlID == Open && !alone)) {
        label(dc, strongFont, current, area, mix(pressed ? text : link, backdrop, disabled ? 40 : 100), DT_CENTER | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
        if (focused) rounded(dc, area, px(6), CLR_INVALID, accent, px(2));
        return;
    }
    // One primary button: Get when the collaborator has a newer version, otherwise Share.
    const bool getFirst = status.state == L"REMOTE_AHEAD" && !confirming;
    const bool primary = item.CtlID == Choose || item.CtlID == Open || (item.CtlID == Share && !getFirst) || (item.CtlID == Get && getFirst);
    std::wstring caption = current;
    if (item.CtlID == Share) caption = confirming ? pick(status, L"Share anyway", L"Yine de paylaş") : pick(status, L"Share current version", L"Güncel sürümü paylaş");
    else if (item.CtlID == Get && confirming) caption = pick(status, L"Cancel", L"Vazgeç");
    // Get opens TrackMesh on this project, hence the ellipsis; the badge already names the collaborator.
    else if (item.CtlID == Get) caption = pick(status, L"Get version…", L"Sürümü al…");
    const auto shade = disabled ? 40 : 100;
    const auto face = primary ? (pressed ? accentHover : accent) : (pressed ? buttonHover : buttonFill);
    rounded(dc, area, px(6), mix(face, backdrop, shade), mix(primary ? accent : buttonBorder, backdrop, shade));
    // A lighter top edge and a darker bottom edge, 1 px each, give the one green button a little depth, as `.primary` does in the app.
    if (primary && !disabled) {
        for (const auto& [y, color] : {std::pair{area.top + 1, accentEdge}, std::pair{area.bottom - 2, accentShade}}) {
            const auto pen = CreatePen(PS_SOLID, 1, color);
            const auto oldPen = SelectObject(dc, pen);
            MoveToEx(dc, area.left + px(6), y, nullptr);
            LineTo(dc, area.right - px(6), y);
            SelectObject(dc, oldPen);
            DeleteObject(pen);
        }
    }
    label(dc, strongFont, caption, {area.left + px(8), area.top, area.right - px(8), area.bottom}, mix(primary ? accentInk : text, backdrop, shade),
        DT_CENTER | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
    if (focused) rounded(dc, {area.left + px(2), area.top + px(2), area.right - px(2), area.bottom - px(2)}, px(4), CLR_INVALID, primary ? accentInk : accent, px(2));
}
LRESULT CALLBACK View::procedure(HWND handle, UINT message, WPARAM wparam, LPARAM lparam) {
    auto self = reinterpret_cast<View*>(GetWindowLongPtrW(handle, GWLP_USERDATA));
    if (message == WM_NCCREATE) {
        self = static_cast<View*>(reinterpret_cast<CREATESTRUCTW*>(lparam)->lpCreateParams);
        SetWindowLongPtrW(handle, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(self));
    }
    if (!self) return DefWindowProcW(handle, message, wparam, lparam);
    switch (message) {
        case WM_PAINT: self->paint(handle); return 0;
        case WM_ERASEBKGND: return 1;
        case WM_TIMER: {
            self->refresh();
            return 0;
        }
        case WM_COMMAND:
            // A warned Share takes a second press; the Get button turns into Cancel meanwhile.
            if (HIWORD(wparam) == BN_CLICKED && LOWORD(wparam) == Share) {
                if (self->confirming || self->bridge->status().shareConfirmation.empty()) { self->confirming = false; self->bridge->send(); }
                else self->confirming = true;
                self->refresh();
            }
            if (HIWORD(wparam) == BN_CLICKED && LOWORD(wparam) == Get) {
                if (self->confirming) self->confirming = false;
                else self->bridge->receive();
                self->refresh();
            }
            if (HIWORD(wparam) == BN_CLICKED && LOWORD(wparam) == Open) self->bridge->receive();
            if (HIWORD(wparam) == BN_CLICKED && LOWORD(wparam) == AutoShare)
                self->bridge->setAutoShare(!self->bridge->status().autoShare);
            if (HIWORD(wparam) == BN_CLICKED && LOWORD(wparam) == Change) { self->changing = !self->changing; self->selectionError.clear(); self->refresh(); }
            if (HIWORD(wparam) == LBN_SELCHANGE && LOWORD(wparam) == Picker) self->refresh();
            // A double-click on a project, or Use project, links it.
            if ((HIWORD(wparam) == LBN_DBLCLK && LOWORD(wparam) == Picker) || (HIWORD(wparam) == BN_CLICKED && LOWORD(wparam) == Choose)) {
                if (self->bridge->status().connected) self->chooseSelected();
            }
            return 0;
        case WM_DRAWITEM: {
            const auto item = reinterpret_cast<DRAWITEMSTRUCT*>(lparam);
            if (item->CtlType != ODT_BUTTON && item->CtlType != ODT_LISTBOX) break;
            self->drawItem(*item);
            return TRUE;
        }
        case WM_CTLCOLOREDIT:
        case WM_CTLCOLORLISTBOX:
        case WM_CTLCOLORSTATIC: {
            // The project picker's list and field use the app's input colours.
            SetTextColor(reinterpret_cast<HDC>(wparam), text);
            SetBkColor(reinterpret_cast<HDC>(wparam), inputFill);
            return reinterpret_cast<LRESULT>(self->inputBrush);
        }
        case WM_NCDESTROY: {
            self->window = nullptr;
            SetWindowLongPtrW(handle, GWLP_USERDATA, 0);
            return DefWindowProcW(handle, message, wparam, lparam);
        }
    }
    return DefWindowProcW(handle, message, wparam, lparam);
}
}
