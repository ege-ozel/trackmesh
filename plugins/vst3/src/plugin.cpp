#include "plugin.h"
#include "view.h"
#include "public.sdk/source/main/pluginfactory.h"
#include "pluginterfaces/base/ibstream.h"
#include <algorithm>
#include <cstring>
#include <type_traits>

using namespace Steinberg;
using namespace Steinberg::Vst;
namespace TrackMesh {
namespace {
constexpr unsigned char stateMagic[8] {'T','M','P','R','O','J',1,0};
template<typename Sample> void pass(ProcessData& data) {
    auto& input = data.inputs[0];
    auto& output = data.outputs[0];
    auto source = std::is_same_v<Sample, float> ? reinterpret_cast<Sample**>(input.channelBuffers32)
                                              : reinterpret_cast<Sample**>(input.channelBuffers64);
    auto target = std::is_same_v<Sample, float> ? reinterpret_cast<Sample**>(output.channelBuffers32)
                                              : reinterpret_cast<Sample**>(output.channelBuffers64);
    output.silenceFlags = 0;
    for (int32 channel = 0; channel < output.numChannels; ++channel) {
        const bool silent = channel >= input.numChannels || (input.silenceFlags & (uint64{1} << channel));
        if (silent) {
            std::memset(target[channel], 0, static_cast<size_t>(data.numSamples) * sizeof(Sample));
            output.silenceFlags |= uint64{1} << channel;
        } else if (target[channel] != source[channel]) {
            std::memmove(target[channel], source[channel], static_cast<size_t>(data.numSamples) * sizeof(Sample));
        }
    }
}
}
tresult PLUGIN_API Plugin::initialize(FUnknown* context) {
    const auto result = SingleComponentEffect::initialize(context);
    if (result != kResultOk) return result;
    addAudioInput(STR16("Input"), SpeakerArr::kStereo);
    addAudioOutput(STR16("Output"), SpeakerArr::kStereo);
    try {
        bridge->setProject(newProjectId());
        if (!bridge->start()) return kResultFalse;
    } catch (...) { return kResultFalse; }
    return kResultOk;
}
tresult PLUGIN_API Plugin::terminate() {
    bridge->stop();
    return SingleComponentEffect::terminate();
}
tresult PLUGIN_API Plugin::getState(IBStream* stream) {
    if (!stream) return kInvalidArgument;
    auto project = bridge->project();
    int32 written = 0;
    if (stream->write(const_cast<unsigned char*>(stateMagic), sizeof(stateMagic), &written) != kResultOk || written != sizeof(stateMagic)) return kResultFalse;
    if (stream->write(project.data(), static_cast<int32>(project.size()), &written) != kResultOk || written != project.size()) return kResultFalse;
    // Serialization is an observation, not evidence that DAW disk writes finished.
    unsavedChoice.store(false);
    bridge->saveObserved();
    return kResultOk;
}
tresult PLUGIN_API Plugin::setState(IBStream* stream) {
    if (!stream) return kInvalidArgument;
    unsigned char magic[sizeof(stateMagic)] {};
    ProjectId project {};
    int32 read = 0;
    if (stream->read(magic, sizeof(magic), &read) != kResultOk || read != sizeof(magic) || std::memcmp(magic, stateMagic, sizeof(magic))) return kResultFalse;
    if (stream->read(project.data(), static_cast<int32>(project.size()), &read) != kResultOk || read != project.size() || !validProjectId(project)) return kResultFalse;
    bridge->setProject(project);
    return kResultOk;
}
tresult PLUGIN_API Plugin::setComponentState(IBStream* stream) { return setState(stream); }
tresult PLUGIN_API Plugin::setBusArrangements(SpeakerArrangement* inputs, int32 numInputs, SpeakerArrangement* outputs, int32 numOutputs) {
    if (numInputs != 1 || numOutputs != 1 || !inputs || !outputs || inputs[0] != outputs[0]
        || (inputs[0] != SpeakerArr::kMono && inputs[0] != SpeakerArr::kStereo)) return kResultFalse;
    return SingleComponentEffect::setBusArrangements(inputs, numInputs, outputs, numOutputs);
}
tresult PLUGIN_API Plugin::canProcessSampleSize(int32 size) { return size == kSample32 || size == kSample64 ? kResultTrue : kResultFalse; }
tresult PLUGIN_API Plugin::process(ProcessData& data) {
    if (data.numSamples <= 0) return kResultOk;
    if (data.numInputs != 1 || data.numOutputs != 1 || !data.inputs || !data.outputs) return kResultFalse;
    if (data.inputs[0].numChannels < 1 || data.inputs[0].numChannels > 2
        || data.outputs[0].numChannels != data.inputs[0].numChannels) return kResultFalse;
    const auto& input = data.inputs[0];
    const auto& output = data.outputs[0];
    if (data.symbolicSampleSize == kSample32) {
        if (!input.channelBuffers32 || !output.channelBuffers32) return kInvalidArgument;
        for (int32 channel = 0; channel < output.numChannels; ++channel) {
            if (!output.channelBuffers32[channel] || (!input.channelBuffers32[channel]
                && !(input.silenceFlags & (uint64{1} << channel)))) return kInvalidArgument;
        }
    } else if (data.symbolicSampleSize == kSample64) {
        if (!input.channelBuffers64 || !output.channelBuffers64) return kInvalidArgument;
        for (int32 channel = 0; channel < output.numChannels; ++channel) {
            if (!output.channelBuffers64[channel] || (!input.channelBuffers64[channel]
                && !(input.silenceFlags & (uint64{1} << channel)))) return kInvalidArgument;
        }
    }
    if (data.symbolicSampleSize == kSample32) pass<float>(data);
    else if (data.symbolicSampleSize == kSample64) pass<double>(data);
    else return kResultFalse;
    return kResultOk;
}
IPlugView* PLUGIN_API Plugin::createView(FIDString name) {
    if (name && std::strcmp(name, ViewType::kEditor) == 0) return new View(bridge, this);
    return nullptr;
}
Plugin::Selection Plugin::selectProject(const ProjectId& project) {
    if (!validProjectId(project)) return Selection::Refused;
    bridge->setProject(project);
    // UI thread only: the SDK marks non-parameter component state as dirty. Hosts without
    // IComponentHandler2 (FL Studio 20.6) still store getState on their next save.
    const bool marked = componentHandler2 && setDirty(true) == kResultOk;
    unsavedChoice.store(!marked);
    return marked ? Selection::Saved : Selection::SaveNeeded;
}
}

// Permanent plugin class ID: changing it breaks saved DAW projects.
static const FUID classId(0xAD81AE22, 0x4F924B13, 0x9E7A621C, 0x14B409D8);
BEGIN_FACTORY_DEF("TrackMesh", "", "")
    DEF_CLASS2(INLINE_UID_FROM_FUID(classId), PClassInfo::kManyInstances, kVstAudioEffectClass,
        "TrackMesh", 0, "Fx|Tools", "0.1.0", kVstVersionString, TrackMesh::Plugin::create)
END_FACTORY
