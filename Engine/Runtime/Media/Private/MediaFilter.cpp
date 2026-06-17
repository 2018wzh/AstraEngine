#include <Astra/Media/Media.hpp>

#include <Astra/Asset/Asset.hpp>

#include <sstream>

namespace Astra::Media {

std::string ToString(FilterTarget target);

namespace {

Astra::Core::Diagnostic MakeDiagnostic(std::string code, Astra::Core::DiagnosticSeverity severity, std::string message) {
    Astra::Core::Diagnostic diagnostic;
    diagnostic.code = std::move(code);
    diagnostic.category = "media.foundation";
    diagnostic.severity = severity;
    diagnostic.message = std::move(message);
    return diagnostic;
}

std::string StableHash(std::string_view text) {
    constexpr Astra::Core::u64 offset = 14695981039346656037ull;
    constexpr Astra::Core::u64 prime = 1099511628211ull;
    Astra::Core::u64 value = offset;
    for (const auto character : text) {
        value ^= static_cast<unsigned char>(character);
        value *= prime;
    }
    std::ostringstream output;
    output << std::hex << value;
    return output.str();
}

} // namespace
Astra::Core::Result<FilterProfile> FilterProfileFromJson(const nlohmann::json& json, Astra::Core::DiagnosticSink& diagnostics) {
    auto id = Astra::Asset::ParseAssetUri(json.value("id", ""));
    if (!id) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_FILTER_ID_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "FilterProfile id is invalid."));
        return Astra::Core::Result<FilterProfile>::Failure(id.Error(), id.Message());
    }
    FilterProfile profile;
    profile.schema = json.value("schema", FilterProfileSchema);
    profile.id = id.Value();
    for (const auto& pass_json : json.value("passes", nlohmann::json::array())) {
        auto target = FilterTargetFromString(pass_json.value("target", "final"));
        if (!target) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_FILTER_TARGET_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "FilterProfile pass has invalid target."));
            continue;
        }
        profile.passes.push_back({pass_json.value("id", ""), pass_json.value("filter", ""), target.Value(), pass_json.value("params", nlohmann::json::object())});
    }
    return Astra::Core::Result<FilterProfile>::Success(std::move(profile));
}

Astra::Core::Result<void> ValidateFilterProfile(const FilterProfile& profile, Astra::Core::DiagnosticSink& diagnostics) {
    bool valid = true;
    if (profile.id.path.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_FILTER_ID_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "FilterProfile id is required."));
        valid = false;
    }
    for (const auto& pass : profile.passes) {
        if (pass.id.empty() || pass.filter.empty()) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_FILTER_PASS_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "FilterProfile pass requires id and filter."));
            valid = false;
        }
    }
    return valid ? Astra::Core::Result<void>::Success() : Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "invalid filter profile");
}

std::vector<FilterApplication> ApplyFilterProfile(const FilterProfile& profile) {
    std::vector<FilterApplication> applications;
    for (const auto& pass : profile.passes) {
        applications.push_back({pass.id, pass.filter, pass.target, ToString(pass.target), StableHash(pass.params.dump())});
    }
    return applications;
}

Astra::Core::Result<FilterTarget> FilterTargetFromString(std::string_view value) {
    if (value == "background") {
        return Astra::Core::Result<FilterTarget>::Success(FilterTarget::Background);
    }
    if (value == "character") {
        return Astra::Core::Result<FilterTarget>::Success(FilterTarget::Character);
    }
    if (value == "ui") {
        return Astra::Core::Result<FilterTarget>::Success(FilterTarget::Ui);
    }
    if (value == "text") {
        return Astra::Core::Result<FilterTarget>::Success(FilterTarget::Text);
    }
    if (value == "final") {
        return Astra::Core::Result<FilterTarget>::Success(FilterTarget::Final);
    }
    return Astra::Core::Result<FilterTarget>::Failure(Astra::Core::ErrorCode::InvalidFormat, "unknown filter target");
}


} // namespace Astra::Media
