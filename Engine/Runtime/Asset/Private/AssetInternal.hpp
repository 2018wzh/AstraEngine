#pragma once

#include <Astra/Core/Diagnostics.hpp>
#include <Astra/Core/Types.hpp>

#include <nlohmann/json.hpp>
#include <yaml-cpp/yaml.h>

#include <filesystem>
#include <span>
#include <string>
#include <string_view>
#include <vector>

namespace Astra::Asset {

[[nodiscard]] ASTRA_ASSET_API std::string NormalizePath(std::string_view value);
[[nodiscard]] ASTRA_ASSET_API Astra::Core::Diagnostic MakeDiagnostic(std::string code, Astra::Core::DiagnosticSeverity severity, std::string message, const std::filesystem::path& path = {});
[[nodiscard]] ASTRA_ASSET_API std::string ReadText(const std::filesystem::path& path);
[[nodiscard]] ASTRA_ASSET_API std::vector<Astra::Core::u8> ReadBytes(const std::filesystem::path& path);
ASTRA_ASSET_API void WriteBytes(const std::filesystem::path& path, std::span<const Astra::Core::u8> bytes);
[[nodiscard]] ASTRA_ASSET_API std::string LowerExtension(const std::filesystem::path& path);
[[nodiscard]] ASTRA_ASSET_API std::string Sha256Text(std::string_view text);
[[nodiscard]] ASTRA_ASSET_API Astra::Core::Result<std::vector<Astra::Core::u8>> DecodeBase64(std::string_view text);
[[nodiscard]] ASTRA_ASSET_API std::string Sha256Bytes(const std::vector<Astra::Core::u8>& bytes);
[[nodiscard]] ASTRA_ASSET_API std::string Sha256Span(std::span<const Astra::Core::u8> bytes);
[[nodiscard]] ASTRA_ASSET_API std::string Sha256File(const std::filesystem::path& path);
[[nodiscard]] ASTRA_ASSET_API std::vector<Astra::Core::u8> CompressZstd(std::span<const Astra::Core::u8> bytes);
[[nodiscard]] ASTRA_ASSET_API Astra::Core::Result<std::vector<Astra::Core::u8>> DecompressZstd(std::span<const Astra::Core::u8> bytes, Astra::Core::u64 expected_size);
[[nodiscard]] ASTRA_ASSET_API Astra::Core::Result<nlohmann::json> ReadPackageJson(const std::filesystem::path& package_path, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_ASSET_API nlohmann::json YamlToJson(const YAML::Node& node);
[[nodiscard]] ASTRA_ASSET_API nlohmann::json InspectImageCookMetadata(std::span<const Astra::Core::u8> bytes, const std::filesystem::path& source_path);
[[nodiscard]] ASTRA_ASSET_API nlohmann::json InspectFontCookMetadata(std::span<const Astra::Core::u8> bytes);
[[nodiscard]] ASTRA_ASSET_API nlohmann::json InspectAudioCookMetadata(std::span<const Astra::Core::u8> bytes, const std::filesystem::path& source_path);
[[nodiscard]] ASTRA_ASSET_API std::string ToString(HotReloadStage stage);

} // namespace Astra::Asset
