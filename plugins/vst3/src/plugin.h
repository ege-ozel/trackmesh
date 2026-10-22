#pragma once
#include "bridge.h"
#include <atomic>
#include "public.sdk/source/vst/vstsinglecomponenteffect.h"

namespace TrackMesh {
class Plugin final : public Steinberg::Vst::SingleComponentEffect {
public:
    static Steinberg::FUnknown* create(void*) { return static_cast<Steinberg::Vst::IComponent*>(new Plugin); }
    Steinberg::tresult PLUGIN_API initialize(Steinberg::FUnknown* context) override;
    Steinberg::tresult PLUGIN_API terminate() override;
    Steinberg::tresult PLUGIN_API getState(Steinberg::IBStream* stream) override;
    Steinberg::tresult PLUGIN_API setState(Steinberg::IBStream* stream) override;
    Steinberg::tresult PLUGIN_API setComponentState(Steinberg::IBStream* stream) override;
    Steinberg::tresult PLUGIN_API setBusArrangements(Steinberg::Vst::SpeakerArrangement* inputs, Steinberg::int32 numInputs,
        Steinberg::Vst::SpeakerArrangement* outputs, Steinberg::int32 numOutputs) override;
    Steinberg::tresult PLUGIN_API canProcessSampleSize(Steinberg::int32 size) override;
    Steinberg::tresult PLUGIN_API setProcessing(Steinberg::TBool) override { return Steinberg::kResultOk; }
    Steinberg::tresult PLUGIN_API process(Steinberg::Vst::ProcessData& data) override;
    Steinberg::IPlugView* PLUGIN_API createView(Steinberg::FIDString name) override;
    enum class Selection { Refused, Saved, SaveNeeded };
    Selection selectProject(const ProjectId& project);
    // The host could not mark the project changed; cleared when the host next asks for the state.
    bool saveNeeded() const { return unsavedChoice.load(); }
private:
    std::shared_ptr<Bridge> bridge {std::make_shared<Bridge>()};
    std::atomic<bool> unsavedChoice {false};
};
}
