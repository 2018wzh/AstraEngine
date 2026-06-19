#include <Astra/Asset/Asset.hpp>
#include <Astra/AstraVN/AstraVN.hpp>
#include <Astra/AstraGame/AstraGame.hpp>
#include <Astra/Core/BuildInfo.hpp>
#include <Astra/Core/Config.hpp>
#include <Astra/Core/Diagnostics.hpp>
#include <Astra/Core/Error.hpp>
#include <Astra/Core/Logging.hpp>
#include <Astra/Core/Path.hpp>
#include <Astra/Core/Profiling.hpp>
#include <Astra/Core/Serialization.hpp>
#include <Astra/Core/StableId.hpp>
#include <Astra/Core/Time.hpp>
#include <Astra/Core/Types.hpp>
#include <Astra/Media/Media.hpp>
#include <Astra/ModuleRuntime/ModuleRuntime.hpp>
#include <Astra/Platform/Platform.hpp>
#include <Astra/PropertySystem/PropertySystem.hpp>
#include <Astra/Runtime/Runtime.hpp>
#include <Astra/Scene/Scene.hpp>
#include <Astra/Script/Script.hpp>

#if defined(ASTRA_WITH_TOOLS)
#include <Astra/Tools/Tools.hpp>
#endif

#include <catch2/catch_test_macros.hpp>

#include <filesystem>
#include <fstream>
#include <iterator>
#include <vector>

namespace {

std::vector<Astra::Core::u8> TestPng1x1Rgba() {
    return {
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
        0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
        0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78,
        0xda, 0x63, 0xf8, 0xff, 0xff, 0x3f, 0x00, 0x05, 0xfe, 0x02, 0xfe, 0xa7, 0x35, 0x81,
        0x84, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    };
}

std::vector<Astra::Core::u8> ReadFixtureBytes(const std::filesystem::path& path) {
    std::ifstream file(path, std::ios::binary);
    return {std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>()};
}

} // namespace

#include "Phases/Phase3_Core.cpp"

#include "Phases/Phase4_Platform.cpp"

#include "Phases/Phase5_ModuleRuntime.cpp"

#include "Phases/Phase6_Property.cpp"

#include "Phases/Phase7_Scene.cpp"

#include "Phases/Phase8_Runtime.cpp"

#include "Phases/Phase9_SaveLoadReplay.cpp"

#include "Phases/Phase10_Asset.cpp"

#include "Phases/Phase11_Media.cpp"

#include "Phases/Phase12_Script.cpp"

#include "Phases/Phase13_AstraVN.cpp"

#include "Phases/Phase18_AstraGame.cpp"

#if defined(ASTRA_WITH_TOOLS)
#include "Phases/Phase17_Tools.cpp"
#endif
