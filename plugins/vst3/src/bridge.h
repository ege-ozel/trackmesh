#pragma once
#include <array>
#include <atomic>
#include <memory>
#include <mutex>
#include <string>
#include <vector>
#include <windows.h>

namespace TrackMesh {
using ProjectId = std::array<unsigned char, 16>;
struct ProjectChoice {
    ProjectId id {};
    std::wstring name, folder;
};
struct ViewStatus {
    std::wstring project, name, local, remote, author, createdAt, connection, state, message;
    bool connected {false};
    bool linked {false};
    bool peerOnline {false};
    std::wstring line;
    std::vector<ProjectChoice> projects;
    bool pickerSupported {false}, projectsMore {false}, desktopAvailable {false};
    bool shareEnabled {false}, getEnabled {false};
    bool autoShareSupported {false}, autoShare {false};
    bool presenceDanger {false};
    bool refused {false}; // the agent answered ok:false, e.g. a share that did not finish
    std::wstring presenceWarning, shareConfirmation;
    // The Desktop language ("tr" or "en") and the Get button text; older agents send neither.
    std::wstring language, getLabel;
    // Every collaborator for the badge (the online ones when any is); older agents send only `author`.
    std::wstring collaborators;
};
struct BridgeState {
    std::mutex mutex;
    ProjectId project {};
    ViewStatus view;
    // Last language the agent sent, kept while it is unreachable. Until TrackMesh first answers, Windows' display
    // language decides, as the Desktop does on its first start; otherwise "TrackMesh not reachable" was always English.
    std::wstring language {PRIMARYLANGID(GetUserDefaultUILanguage()) == LANG_TURKISH ? L"tr" : L"en"};
    std::atomic<uint64_t> generation {0}, save {0}, send {0}, receive {0};
    std::atomic<uint64_t> autoShareChange {0};
    std::atomic<bool> autoShareValue {false};
    std::atomic<bool> ending {false};
    HANDLE wake {nullptr};
    BridgeState();
    ~BridgeState();
};
class Bridge {
public:
    Bridge();
    ~Bridge();
    bool start();
    void stop() noexcept;
    void setProject(const ProjectId& project);
    ProjectId project() const;
    void saveObserved() noexcept;
    void send() noexcept;
    void receive() noexcept;
    void setAutoShare(bool enabled);
    ViewStatus status() const;
private:
    std::shared_ptr<BridgeState> state;
    bool started {false};
};
ProjectId newProjectId();
bool validProjectId(const ProjectId& project);
ProjectId parseProjectId(const std::string& text);
std::string projectText(const ProjectId& project);
std::wstring wide(const std::string& text);
}
