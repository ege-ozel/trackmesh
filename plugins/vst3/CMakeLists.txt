cmake_minimum_required(VERSION 3.25)
project(TrackMeshVST3 VERSION 0.1.0 LANGUAGES CXX)
if(NOT WIN32 OR NOT MSVC OR NOT CMAKE_SIZEOF_VOID_P EQUAL 8)
    message(FATAL_ERROR "TrackMesh MVP requires Windows x64 and MSVC")
endif()
# The SDK adds /EHa to its own targets in Release; CMake's default /EHsc would only be overridden
# there (warning D9025). TrackMesh itself keeps /EHsc below.
string(REPLACE "/EHsc" "" CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS}")
add_compile_options($<$<NOT:$<CONFIG:Release>>:/EHsc>)
set(TRACKMESH_VST3_SDK "${CMAKE_CURRENT_SOURCE_DIR}/../../vendor/vst3sdk" CACHE PATH "Official Steinberg VST3 SDK v3.8.0_build_66")
set(TRACKMESH_AGENT "${CMAKE_CURRENT_SOURCE_DIR}/../../target/debug/trackmesh-agent.exe" CACHE FILEPATH "Built Rust agent copied beside the plugin binary")
set(TRACKMESH_DESKTOP "${CMAKE_CURRENT_SOURCE_DIR}/../../target/debug/trackmesh-desktop.exe" CACHE FILEPATH "Built desktop/agent owner copied beside the plugin binary")
set(SMTG_ENABLE_VST3_PLUGIN_EXAMPLES OFF CACHE BOOL "" FORCE)
option(TRACKMESH_BUILD_EDITOR_HOST "Build the SDK editor host for manual native inspection" OFF)
set(SMTG_ENABLE_VST3_HOSTING_EXAMPLES ${TRACKMESH_BUILD_EDITOR_HOST} CACHE BOOL "" FORCE)
set(SMTG_ENABLE_VSTGUI_SUPPORT OFF CACHE BOOL "" FORCE)
set(SMTG_RUN_VST_VALIDATOR OFF CACHE BOOL "No automated test suites in this MVP" FORCE)
set(SMTG_CREATE_PLUGIN_LINK OFF CACHE BOOL "Do not modify installed VST directories" FORCE)
add_subdirectory("${TRACKMESH_VST3_SDK}" "${CMAKE_BINARY_DIR}/sdk")
# SDK variables are directory scoped; our target lives outside the SDK directory.
set(public_sdk_SOURCE_DIR "${TRACKMESH_VST3_SDK}/public.sdk")
smtg_add_vst3plugin(TrackMesh
    src/plugin.cpp src/plugin.h src/bridge.cpp src/bridge.h src/view.cpp src/view.h
    "${TRACKMESH_VST3_SDK}/public.sdk/source/vst/vstsinglecomponenteffect.cpp"
)
# SDK 3.8's Windows package helper sets only multi-config output directories.
# Keep the same bundle layout with single-config Ninja as well.
get_target_property(TRACKMESH_PACKAGE TrackMesh SMTG_PLUGIN_PACKAGE_PATH)
set_target_properties(TrackMesh PROPERTIES LIBRARY_OUTPUT_DIRECTORY "${TRACKMESH_PACKAGE}/Contents/x86_64-win")
target_compile_features(TrackMesh PRIVATE cxx_std_17)
target_compile_definitions(TrackMesh PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN UNICODE _UNICODE)
target_compile_options(TrackMesh PRIVATE /EHsc /W4 /WX /permissive- /utf-8)
target_link_libraries(TrackMesh PRIVATE sdk advapi32 ole32 user32 gdi32 uxtheme)
if(NOT EXISTS "${TRACKMESH_AGENT}" OR NOT EXISTS "${TRACKMESH_DESKTOP}")
    message(FATAL_ERROR "Build trackmesh-core and trackmesh-desktop first, or set TRACKMESH_AGENT and TRACKMESH_DESKTOP")
endif()
add_custom_target(trackmesh_vst3_owners
    COMMAND ${CMAKE_COMMAND} -E make_directory "$<TARGET_FILE_DIR:TrackMesh>"
    COMMAND ${CMAKE_COMMAND} -E make_directory "${TRACKMESH_PACKAGE}/Contents/Resources"
    COMMAND ${CMAKE_COMMAND} -E copy_if_different "${TRACKMESH_AGENT}" "$<TARGET_FILE_DIR:TrackMesh>/trackmesh-agent.exe"
    COMMAND ${CMAKE_COMMAND} -E copy_if_different "${TRACKMESH_DESKTOP}" "$<TARGET_FILE_DIR:TrackMesh>/trackmesh-desktop.exe"
    COMMAND ${CMAKE_COMMAND} -E copy_if_different "${TRACKMESH_VST3_SDK}/LICENSE.txt" "${TRACKMESH_PACKAGE}/Contents/Resources/Steinberg-SDK-LICENSE.txt"
    VERBATIM
)
add_dependencies(TrackMesh trackmesh_vst3_owners)
