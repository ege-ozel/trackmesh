#pragma once
#include "bridge.h"
#include "public.sdk/source/common/pluginview.h"

namespace TrackMesh {
class Plugin;
struct Tone;
class View final : public Steinberg::CPluginView {
public:
    View(std::shared_ptr<Bridge> bridge, Plugin* plugin);
    ~View() override;
    Steinberg::tresult PLUGIN_API isPlatformTypeSupported(Steinberg::FIDString type) override;
    Steinberg::tresult PLUGIN_API attached(void* parent, Steinberg::FIDString type) override;
    Steinberg::tresult PLUGIN_API removed() override;
private:
    static LRESULT CALLBACK procedure(HWND window, UINT message, WPARAM wparam, LPARAM lparam);
    void paint(HWND window);
    void refresh();
    void drawItem(const DRAWITEMSTRUCT& item);
    void notice(HDC dc, RECT area, const Tone& tone, const std::wstring& value, HFONT textFont, COLORREF ink) const;
    void place(HWND control, RECT area);
    RECT pickerArea() const; // the visible project list: one 36 px row per project, at most five before it scrolls
    void chooseSelected();
    std::shared_ptr<Bridge> bridge;
    Plugin* plugin;
    std::vector<ProjectChoice> choices;
    bool changing {false};
    bool confirming {false}; // Share asked for a second press after a warning
    int dpi {96};
    int px(int value) const { return MulDiv(value, dpi, 96); }
    RECT box(int left, int top, int right, int bottom) const { return {px(left), px(top), px(right), px(bottom)}; }
    void makeFonts();
    std::wstring selectionError;
    std::wstring drawnLabels; // language, Get label and share-on-save state the owner-drawn controls were painted with
    HWND window {nullptr}, sendButton {nullptr}, receiveButton {nullptr};
    HWND picker {nullptr}, chooseButton {nullptr}, changeButton {nullptr}, openButton {nullptr};
    HWND autoShareCheckbox {nullptr};
    HFONT font {nullptr}, titleFont {nullptr}, smallFont {nullptr}, strongFont {nullptr}, badgeFont {nullptr};
    HBRUSH inputBrush {nullptr}; // the project list's background below its rows
    HMODULE viewModule {nullptr};
    bool registeredClass {false};
};
}
