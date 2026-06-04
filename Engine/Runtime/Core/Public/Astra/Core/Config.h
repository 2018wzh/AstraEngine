#pragma once

#include <Astra/Core/Diagnostics.h>
#include <Astra/Core/Result.h>

#include <filesystem>
#include <nlohmann/json.hpp>
#include <string>

namespace astra {

Expected<nlohmann::json> load_yaml_file_as_json(const std::filesystem::path& path,
                                                DiagnosticSink& diagnostics);
Expected<nlohmann::json> load_json_file(const std::filesystem::path& path,
                                        DiagnosticSink& diagnostics);

VoidResult validate_json_schema(const nlohmann::json& value, const nlohmann::json& schema,
                                DiagnosticSink& diagnostics,
                                std::string path = "$");

VoidResult validate_yaml_file_with_schema(const std::filesystem::path& yaml_path,
                                          const std::filesystem::path& schema_path,
                                          DiagnosticSink& diagnostics);

} // namespace astra
