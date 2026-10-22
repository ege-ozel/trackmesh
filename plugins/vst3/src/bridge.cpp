#include "bridge.h"
#include <ctime>
#include "public.sdk/source/vst/moduleinfo/json.h"
#include <sddl.h>
#include <objbase.h>
#include <algorithm>
#include <cstdlib>
#include <filesystem>
#include <stdexcept>
#include <vector>
#include <optional>

namespace TrackMesh {
namespace {
constexpr DWORD maxFrame = 64 * 1024;
struct Handle {
    HANDLE value {INVALID_HANDLE_VALUE};
    explicit Handle(HANDLE h) : value(h) {}
    ~Handle() { if (value != INVALID_HANDLE_VALUE && value) CloseHandle(value); }
    Handle(const Handle&) = delete;
    Handle& operator=(const Handle&) = delete;
};
struct Work { std::shared_ptr<BridgeState> state; HMODULE module; };
std::wstring environment(const wchar_t* name) {
    const auto size = GetEnvironmentVariableW(name, nullptr, 0);
    if (size == 0 || size > 32768) return {};
    std::wstring value(size, L'\0');
    const auto read = GetEnvironmentVariableW(name, value.data(), size);
    if (!read || read >= size) return {};
    value.resize(read);
    return value;
}
std::wstring pipeName() {
    HANDLE raw = nullptr;
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw)) throw std::runtime_error("Windows user token unavailable");
    Handle token(raw);
    DWORD size = 0;
    GetTokenInformation(token.value, TokenUser, nullptr, 0, &size);
    if (!size || size > maxFrame) throw std::runtime_error("Invalid Windows token");
    std::vector<uintptr_t> buffer((size + sizeof(uintptr_t) - 1) / sizeof(uintptr_t));
    if (!GetTokenInformation(token.value, TokenUser, buffer.data(), size, &size)) throw std::runtime_error("Windows SID unavailable");
    wchar_t* text = nullptr;
    if (!ConvertSidToStringSidW(reinterpret_cast<TOKEN_USER*>(buffer.data())->User.Sid, &text)) throw std::runtime_error("Windows SID unavailable");
    const std::wstring sid(text);
    LocalFree(text);
    const auto suffix = environment(L"TRACKMESH_IPC_SUFFIX");
    if (suffix.size() > 32 || !std::all_of(suffix.begin(), suffix.end(), [](wchar_t c) {
        return (c >= L'a' && c <= L'z') || (c >= L'A' && c <= L'Z') || (c >= L'0' && c <= L'9') || c == L'-';
    })) throw std::runtime_error("Invalid IPC suffix");
    return L"\\\\.\\pipe\\TrackMesh-v1-" + sid + L"-" + suffix;
}
void offline(const std::shared_ptr<BridgeState>& state, const wchar_t* message, const wchar_t* turkish) {
    std::lock_guard<std::mutex> lock(state->mutex);
    state->view.connected = false;
    state->view.linked = false;
    state->view.connection = L"OFFLINE";
    state->view.state = L"AGENT UNAVAILABLE";
    state->view.language = state->language;
    state->view.message = state->language == L"tr" ? turkish : message;
}
bool launchAgent(HMODULE module) {
#ifndef NDEBUG
    // Development builds only: release plugins start the installed app and nothing else.
    auto path = environment(L"TRACKMESH_AGENT_PATH");
#else
    std::wstring path;
#endif
    if (path.empty()) {
        std::wstring binary(32768, L'\0');
        const auto size = GetModuleFileNameW(module, binary.data(), static_cast<DWORD>(binary.size()));
        if (!size || size >= binary.size()) return false;
        binary.resize(size);
        path = (std::filesystem::path(binary).parent_path() / L"trackmesh-desktop.exe").wstring();
        // Installer layout: the plugin lives in Common Files\VST3, the app in Program Files\TrackMesh (Windows paths ignore case, so older TRACKMESH installs match).
        const auto programs = environment(L"ProgramW6432");
        if (!std::filesystem::is_regular_file(path) && !programs.empty())
            path = (std::filesystem::path(programs) / L"TrackMesh" / L"trackmesh-desktop.exe").wstring();
    }
    if (!std::filesystem::path(path).is_absolute() || !std::filesystem::is_regular_file(path)) return false;
    auto command = L"\"" + path + L"\"";
    const auto filename = std::filesystem::path(path).filename().wstring();
    if (_wcsicmp(filename.c_str(), L"trackmesh-agent.exe") == 0) command += L" plugin-serve";
    else if (_wcsicmp(filename.c_str(), L"trackmesh-desktop.exe") == 0) command += L" --plugin-owner";
    else return false;
    STARTUPINFOW startup {};
    startup.cb = sizeof(startup);
    PROCESS_INFORMATION process {};
    if (!CreateProcessW(path.c_str(), command.data(), nullptr, nullptr, FALSE,
        CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP, nullptr, nullptr, &startup, &process)) return false;
    CloseHandle(process.hThread);
    CloseHandle(process.hProcess);
    return true;
}
// All operations run on the IPC worker. Cancellation always drains the OVERLAPPED
// before its stack buffer/event can disappear. Host callbacks never join this worker.
bool transfer(HANDLE pipe, void* bytes, DWORD size, bool writing) {
    Handle event(CreateEventW(nullptr, TRUE, FALSE, nullptr));
    if (!event.value) return false;
    auto cursor = static_cast<unsigned char*>(bytes);
    const auto deadline = GetTickCount64() + 2000;
    while (size) {
        const auto now = GetTickCount64();
        if (now >= deadline) return false;
        OVERLAPPED operation {};
        operation.hEvent = event.value;
        ResetEvent(event.value);
        DWORD done = 0;
        const BOOL complete = writing ? WriteFile(pipe, cursor, size, &done, &operation)
                                      : ReadFile(pipe, cursor, size, &done, &operation);
        if (!complete) {
            if (GetLastError() != ERROR_IO_PENDING) return false;
            if (WaitForSingleObject(event.value, static_cast<DWORD>(deadline - now)) != WAIT_OBJECT_0) {
                CancelIoEx(pipe, &operation);
                GetOverlappedResult(pipe, &operation, &done, TRUE);
                return false;
            }
            if (!GetOverlappedResult(pipe, &operation, &done, FALSE)) return false;
        }
        if (!done || done > size) return false;
        cursor += done;
        size -= done;
    }
    return true;
}
std::string jsonString(json_object_s* object, const char* key) {
    for (auto el = object->start; el; el = el->next) {
        if (std::string_view(el->name->string, el->name->string_size) != key) continue;
        if (el->value->type == json_type_null) return {};
        if (el->value->type != json_type_string) throw std::runtime_error("Invalid IPC status field");
        const auto text = static_cast<json_string_s*>(el->value->payload);
        if (text->string_size > 8192) throw std::runtime_error("IPC status field too large");
        return {text->string, text->string_size};
    }
    throw std::runtime_error("Missing IPC status field");
}
json_value_s* field(json_object_s* object, const char* key) {
    for (auto el = object->start; el; el = el->next)
        if (std::string_view(el->name->string, el->name->string_size) == key) return el->value;
    return nullptr;
}
bool optionalBool(json_object_s* object, const char* key) {
    const auto value = field(object, key);
    if (!value) return false;
    if (value->type == json_type_true) return true;
    if (value->type == json_type_false) return false;
    throw std::runtime_error("Invalid IPC status boolean");
}
std::vector<ProjectChoice> projectChoices(json_object_s* object) {
    const auto value = field(object, "projects");
    if (!value) return {};
    if (value->type != json_type_array) throw std::runtime_error("Invalid project picker");
    const auto array = static_cast<json_array_s*>(value->payload);
    if (array->length > 64) throw std::runtime_error("Project picker too large");
    std::vector<ProjectChoice> result;
    for (auto entry = array->start; entry; entry = entry->next) {
        if (entry->value->type != json_type_object) throw std::runtime_error("Invalid project choice");
        const auto item = static_cast<json_object_s*>(entry->value->payload);
        result.push_back({parseProjectId(jsonString(item, "project_id")),
            wide(jsonString(item, "name")), wide(jsonString(item, "folder_name"))});
    }
    return result;
}
bool jsonBool(json_object_s* object, const char* key) {
    for (auto el = object->start; el; el = el->next) {
        if (std::string_view(el->name->string, el->name->string_size) != key) continue;
        if (el->value->type == json_type_true) return true;
        if (el->value->type == json_type_false) return false;
        break;
    }
    throw std::runtime_error("Invalid IPC status boolean");
}
bool request(HANDLE pipe, const char* event, const std::string& session, const ProjectId& project,
             const std::shared_ptr<BridgeState>& state, std::optional<bool> autoShare = std::nullopt) {
    const auto id = projectText(project);
    auto payload = std::string("{\"event\":\"") + event + "\",\"session_id\":\"" + session + "\",\"project_id\":\"" + id + "\"}";
    if (autoShare.has_value()) {
        payload.pop_back();
        payload += autoShare.value() ? ",\"enabled\":true}" : ",\"enabled\":false}";
    }
    auto length = static_cast<DWORD>(payload.size());
    if (!transfer(pipe, &length, sizeof(length), true) || !transfer(pipe, payload.data(), length, true)) return false;
    if (!transfer(pipe, &length, sizeof(length), false) || !length || length > maxFrame) return false;
    std::string reply(length, '\0');
    if (!transfer(pipe, reply.data(), length, false)) return false;
    std::unique_ptr<json_value_s, decltype(&std::free)> document(json_parse(reply.data(), reply.size()), &std::free);
    if (!document || document->type != json_type_object) return false;
    auto object = static_cast<json_object_s*>(document->payload);
    if (jsonString(object, "project_id") != id) return false;
    ViewStatus view;
    view.project = wide(id);
    view.name = wide(jsonString(object, "project_name"));
    view.local = wide(jsonString(object, "local_snapshot"));
    view.remote = wide(jsonString(object, "remote_snapshot"));
    view.author = wide(jsonString(object, "author"));
    view.createdAt = wide(jsonString(object, "created_at"));
    view.connection = wide(jsonString(object, "connection"));
    view.state = wide(jsonString(object, "state"));
    view.message = wide(jsonString(object, "message"));
    view.pickerSupported = optionalBool(object, "picker_supported");
    view.autoShareSupported = optionalBool(object, "auto_share_supported");
    view.autoShare = optionalBool(object, "auto_share");
    view.presenceDanger = optionalBool(object, "presence_danger");
    if (field(object, "presence_warning")) view.presenceWarning = wide(jsonString(object, "presence_warning"));
    if (field(object, "share_confirmation")) view.shareConfirmation = wide(jsonString(object, "share_confirmation"));
    if (field(object, "language")) view.language = wide(jsonString(object, "language"));
    if (field(object, "get_label")) view.getLabel = wide(jsonString(object, "get_label"));
    if (field(object, "collaborators")) view.collaborators = wide(jsonString(object, "collaborators"));
    const auto since = field(object, "presence_since");
    if (since && since->type == json_type_number && !view.presenceWarning.empty() && !view.presenceDanger) {
        const auto number = static_cast<json_number_s*>(since->payload);
        const auto seconds = static_cast<__time64_t>(std::stoull(std::string(number->number, number->number_size)));
        tm local {};
        if (_localtime64_s(&local, &seconds) == 0) {
            wchar_t time[16] {};
            if (wcsftime(time, 16, L"%H:%M", &local))
                view.presenceWarning += (view.language == L"tr" ? L" (başlangıç " : L" (since ") + std::wstring(time) + L")";
        }
    }
    if (view.pickerSupported) {
        view.projects = projectChoices(object);
        view.projectsMore = optionalBool(object, "projects_more");
        view.desktopAvailable = optionalBool(object, "desktop_available");
        view.shareEnabled = optionalBool(object, "share_enabled");
        view.getEnabled = optionalBool(object, "get_enabled");
        view.line = wide(jsonString(object, "status_line"));
    } else {
        view.line = view.language == L"tr" ? L"Bağlı bir proje seçmek için TrackMesh'i güncelle." : L"Update TrackMesh to choose a linked project.";
        view.message.clear();
    }
    view.connected = true;
    view.linked = !view.name.empty();
    const bool ok = jsonBool(object, "ok");
    view.peerOnline = jsonBool(object, "peer_online");
    view.refused = !ok;
    if (!ok && view.state.empty()) view.state = L"ACTION REFUSED";
    std::lock_guard<std::mutex> lock(state->mutex);
    if (!view.language.empty()) state->language = view.language;
    if (state->project == project) state->view = std::move(view);
    return true;
}
ProjectId projectOf(const std::shared_ptr<BridgeState>& state) {
    std::lock_guard<std::mutex> lock(state->mutex);
    return state->project;
}
std::pair<ProjectId, uint64_t> bindingOf(const std::shared_ptr<BridgeState>& state) {
    std::lock_guard<std::mutex> lock(state->mutex);
    return {state->project, state->generation.load()};
}
void run(Work& work) {
    const auto state = work.state;
    const auto name = pipeName();
    const auto session = projectText(newProjectId());
    ULONGLONG lastLaunch = 0;
    bool everConnected = false;
    ULONGLONG endingDeadline = 0;
    // Termination returns promptly; this pinned worker gets bounded time to hand
    // off queued save/send observations before ending the Rust-owned session.
    auto expired = [&]() {
        if (state->ending.load() && !endingDeadline) endingDeadline = GetTickCount64() + 30000;
        return endingDeadline && GetTickCount64() >= endingDeadline;
    };
    while (!expired()) {
        auto binding = bindingOf(state);
        auto project = binding.first;
        if (!validProjectId(project)) { WaitForSingleObject(state->wake, 250); continue; }
        Handle pipe(CreateFileW(name.c_str(), GENERIC_READ | GENERIC_WRITE, 0, nullptr, OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION, nullptr));
        if (pipe.value == INVALID_HANDLE_VALUE) {
            const auto error = GetLastError();
            offline(state, L"Waiting for TrackMesh to open…", L"TrackMesh'in açılması bekleniyor…");
            if (!everConnected && (error == ERROR_FILE_NOT_FOUND || error == ERROR_PATH_NOT_FOUND) && GetTickCount64() - lastLaunch >= 10000) {
                launchAgent(work.module);
                lastLaunch = GetTickCount64();
            }
            WaitForSingleObject(state->wake, 500);
            continue;
        }
        if (!request(pipe.value, "PLUGIN_CONNECTED", session, project, state)) continue;
        everConnected = true;
        bool healthy = true;
        while (healthy) {
            if (expired()) return;
            const auto current = bindingOf(state);
            if (current != binding) {
                binding = current;
                project = binding.first;
                healthy = request(pipe.value, "PROJECT_ID", session, project, state);
                if (!healthy) break;
            }
            const char* event = "GET_STATUS";
            std::optional<bool> autoShare;
            std::atomic<uint64_t>* signal = nullptr;
            // Each coalesced signal belongs to the UUID generation at enqueue.
            // A host state restore must never send an old button action to a new project.
            const auto autoShareChange = state->autoShareChange.exchange(0);
            bool supportsAutoShare = false;
            { std::lock_guard<std::mutex> lock(state->mutex); supportsAutoShare = state->view.autoShareSupported; }
            if (autoShareChange == binding.second && supportsAutoShare) {
                event = "SET_AUTO_SHARE"; signal = &state->autoShareChange; autoShare = state->autoShareValue.load();
            }
            else if (state->save.exchange(0) == binding.second) { event = "SAVE_OBSERVED"; signal = &state->save; }
            else if (state->send.exchange(0) == binding.second) { event = "SEND_SNAPSHOT"; signal = &state->send; }
            else if (state->ending.load()) {
                // Acquire the ending signal before the final drain: a save may
                // have arrived between the first exchange and terminate().
                if (state->save.exchange(0) == binding.second) { event = "SAVE_OBSERVED"; signal = &state->save; }
                else if (state->send.exchange(0) == binding.second) { event = "SEND_SNAPSHOT"; signal = &state->send; }
                else {
                    if (request(pipe.value, "SESSION_ENDING", session, project, state)) return;
                    healthy = false;
                    break;
                }
            }
            else if (state->receive.exchange(0) == binding.second) { event = "RECEIVE_SNAPSHOT"; signal = &state->receive; }
            healthy = request(pipe.value, event, session, project, state, autoShare);
            if (!healthy && signal) {
                uint64_t empty = 0;
                signal->compare_exchange_strong(empty, binding.second);
            }
            if (healthy && !state->ending.load()) WaitForSingleObject(state->wake, 500);
        }
        if (!healthy) {
            offline(state, L"Lost the connection to TrackMesh. Reconnecting…", L"TrackMesh ile bağlantı koptu. Yeniden bağlanılıyor…");
            WaitForSingleObject(state->wake, 500);
        }
    }
}
DWORD WINAPI worker(void* argument) {
    std::unique_ptr<Work> work(static_cast<Work*>(argument));
    const auto module = work->module;
    try { run(*work); }
    catch (...) { offline(work->state, L"Can't talk to TrackMesh. Reinstall TrackMesh if this stays.", L"TrackMesh ile konuşulamıyor. Böyle kalırsa TrackMesh'i yeniden kur."); }
    work.reset();
    // The host can release the component immediately: the module remains loaded
    // until all worker C++ objects have been destroyed, including the shared state.
    FreeLibraryAndExitThread(module, 0);
}
}
BridgeState::BridgeState() : wake(CreateEventW(nullptr, FALSE, FALSE, nullptr)) {}
BridgeState::~BridgeState() { if (wake) CloseHandle(wake); }
Bridge::Bridge() : state(std::make_shared<BridgeState>()) {}
Bridge::~Bridge() { stop(); }
bool Bridge::start() {
    if (started) return true;
    if (!state->wake) return false;
    HMODULE module = nullptr;
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
        reinterpret_cast<LPCWSTR>(&worker), &module)) return false;
    auto work = std::make_unique<Work>();
    work->state = state;
    work->module = module;
    const auto thread = CreateThread(nullptr, 0, worker, work.get(), 0, nullptr);
    if (!thread) { FreeLibrary(module); return false; }
    work.release();
    CloseHandle(thread);
    started = true;
    return true;
}
void Bridge::stop() noexcept { state->ending.store(true); if (state->wake) SetEvent(state->wake); }
void Bridge::setProject(const ProjectId& project) {
    { std::lock_guard<std::mutex> lock(state->mutex);
      state->project = project;
      state->generation.fetch_add(1);
      state->view = {};
      state->view.project = wide(projectText(project));
      state->view.language = state->language;
      state->view.message = state->language == L"tr" ? L"TrackMesh'e bağlanılıyor…" : L"Connecting to TrackMesh…"; }
    SetEvent(state->wake);
}
ProjectId Bridge::project() const { return projectOf(state); }
void Bridge::saveObserved() noexcept { state->save.store(state->generation.load()); SetEvent(state->wake); }
void Bridge::send() noexcept { state->send.store(state->generation.load()); SetEvent(state->wake); }
void Bridge::receive() noexcept { state->receive.store(state->generation.load()); SetEvent(state->wake); }
void Bridge::setAutoShare(bool enabled) {
    std::lock_guard<std::mutex> lock(state->mutex);
    if (!state->view.connected || !state->view.linked || !state->view.autoShareSupported) return;
    state->autoShareValue.store(enabled);
    state->autoShareChange.store(state->generation.load());
    SetEvent(state->wake);
}
ViewStatus Bridge::status() const { std::lock_guard<std::mutex> lock(state->mutex); return state->view; }
ProjectId newProjectId() {
    GUID guid {};
    if (FAILED(CoCreateGuid(&guid))) throw std::runtime_error("Cannot create project UUID");
    const auto bytes = reinterpret_cast<const unsigned char*>(&guid);
    ProjectId id {};
    std::copy(bytes, bytes + id.size(), id.begin());
    id[6] = static_cast<unsigned char>((id[6] & 0x0f) | 0x40);
    id[8] = static_cast<unsigned char>((id[8] & 0x3f) | 0x80);
    return id;
}
bool validProjectId(const ProjectId& project) { return std::any_of(project.begin(), project.end(), [](unsigned char c) { return c != 0; }); }
ProjectId parseProjectId(const std::string& text) {
    if (text.size() != 36) throw std::runtime_error("Invalid project identifier");
    const auto digit = [](char c) -> unsigned char {
        if (c >= '0' && c <= '9') return static_cast<unsigned char>(c - '0');
        if (c >= 'a' && c <= 'f') return static_cast<unsigned char>(c - 'a' + 10);
        throw std::runtime_error("Invalid project identifier");
    };
    ProjectId id {};
    size_t offset = 0;
    for (size_t i = 0; i < id.size(); ++i) {
        if (i == 4 || i == 6 || i == 8 || i == 10) {
            if (text[offset++] != '-') throw std::runtime_error("Invalid project identifier");
        }
        id[i] = static_cast<unsigned char>((digit(text[offset]) << 4) | digit(text[offset + 1]));
        offset += 2;
    }
    if (!validProjectId(id)) throw std::runtime_error("Nil project identifier");
    return id;
}
std::string projectText(const ProjectId& project) {
    constexpr char digits[] = "0123456789abcdef";
    std::string text;
    text.reserve(36);
    for (size_t i = 0; i < project.size(); ++i) {
        if (i == 4 || i == 6 || i == 8 || i == 10) text += '-';
        text += digits[project[i] >> 4]; text += digits[project[i] & 15];
    }
    return text;
}
std::wstring wide(const std::string& text) {
    if (text.empty()) return {};
    const auto length = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, text.data(), static_cast<int>(text.size()), nullptr, 0);
    if (!length) return L"Invalid status text";
    std::wstring result(length, L'\0');
    MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, text.data(), static_cast<int>(text.size()), result.data(), length);
    return result;
}
}
