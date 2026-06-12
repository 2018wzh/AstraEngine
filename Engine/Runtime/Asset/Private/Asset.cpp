#include <Astra/Asset/Asset.hpp>

#include <openssl/evp.h>
#include <yaml-cpp/yaml.h>
#if defined(ASTRA_ASSET_HAS_LIBPNG)
#include <png.h>
#endif
#if defined(ASTRA_ASSET_HAS_LIBJPEG)
#include <jpeglib.h>
#endif
#if defined(ASTRA_ASSET_HAS_LIBWEBP)
#include <webp/decode.h>
#endif
#if defined(ASTRA_ASSET_HAS_FREETYPE)
#include <ft2build.h>
#include FT_FREETYPE_H
#endif
#if defined(ASTRA_ASSET_HAS_HARFBUZZ)
#include <hb.h>
#include <hb-ft.h>
#endif
#if defined(ASTRA_ASSET_HAS_MINIAUDIO)
#define MINIAUDIO_IMPLEMENTATION
#include <miniaudio.h>
#ifdef min
#undef min
#endif
#ifdef max
#undef max
#endif
#endif

#include <algorithm>
#include <array>
#include <csetjmp>
#include <cctype>
#include <cstddef>
#include <cstring>
#include <fstream>
#include <iomanip>
#include <iterator>
#include <limits>
#include <set>
#include <sstream>
#include <zstd.h>

namespace Astra::Asset {

namespace {

std::string NormalizePath(std::string_view value) {
    std::string result(value);
    std::replace(result.begin(), result.end(), '\\', '/');
    while (result.find("//") != std::string::npos) {
        result.replace(result.find("//"), 2, "/");
    }
    if (!result.empty() && result.front() == '/') {
        result.erase(result.begin());
    }
    return result;
}

Astra::Core::Diagnostic MakeDiagnostic(std::string code, Astra::Core::DiagnosticSeverity severity, std::string message, const std::filesystem::path& path = {}) {
    Astra::Core::Diagnostic diagnostic;
    diagnostic.code = std::move(code);
    diagnostic.category = "asset.foundation";
    diagnostic.severity = severity;
    diagnostic.message = std::move(message);
    diagnostic.source.file = path.string();
    return diagnostic;
}

std::string ReadText(const std::filesystem::path& path) {
    std::ifstream file(path, std::ios::binary);
    return std::string(std::istreambuf_iterator<char>(file), {});
}

std::vector<Astra::Core::u8> ReadBytes(const std::filesystem::path& path) {
    std::ifstream file(path, std::ios::binary);
    const std::string bytes(std::istreambuf_iterator<char>(file), {});
    return {bytes.begin(), bytes.end()};
}

void WriteBytes(const std::filesystem::path& path, std::span<const Astra::Core::u8> bytes) {
    if (path.has_parent_path()) {
        std::filesystem::create_directories(path.parent_path());
    }
    std::ofstream file(path, std::ios::binary);
    if (!bytes.empty()) {
        file.write(reinterpret_cast<const char*>(bytes.data()), static_cast<std::streamsize>(bytes.size()));
    }
}

std::string LowerExtension(const std::filesystem::path& path) {
    auto extension = path.extension().string();
    std::ranges::transform(extension, extension.begin(), [](unsigned char character) {
        return static_cast<char>(std::tolower(character));
    });
    return extension;
}

#if defined(ASTRA_ASSET_HAS_LIBPNG)
struct PngMemoryReader {
    std::span<const Astra::Core::u8> bytes;
    std::size_t offset = 0;
};

void PngReadCallback(png_structp png, png_bytep output, png_size_t size) {
    auto* reader = static_cast<PngMemoryReader*>(png_get_io_ptr(png));
    if (reader == nullptr || reader->offset + size > reader->bytes.size()) {
        png_error(png, "PNG input ended unexpectedly");
        return;
    }
    std::memcpy(output, reader->bytes.data() + reader->offset, size);
    reader->offset += size;
}
#endif

#if defined(ASTRA_ASSET_HAS_LIBJPEG)
struct JpegErrorHandler {
    jpeg_error_mgr base;
    jmp_buf jump;
};

void JpegErrorExit(j_common_ptr cinfo) {
    auto* error = reinterpret_cast<JpegErrorHandler*>(cinfo->err);
    longjmp(error->jump, 1);
}
#endif

nlohmann::json InspectImageCookMetadata(std::span<const Astra::Core::u8> bytes, const std::filesystem::path& source_path) {
    const auto extension = LowerExtension(source_path);
    nlohmann::json metadata = {
        {"schema", "astra.asset.image_cook_metadata.v1"},
        {"status", "unsupported"},
        {"source_extension", extension},
    };
    if (extension == ".png") {
#if defined(ASTRA_ASSET_HAS_LIBPNG)
        if (bytes.size() < 8 || png_sig_cmp(const_cast<png_bytep>(bytes.data()), 0, 8) != 0) {
            metadata["status"] = "invalid";
            metadata["decoded_by"] = "libpng";
            return metadata;
        }
        png_structp png = png_create_read_struct(PNG_LIBPNG_VER_STRING, nullptr, nullptr, nullptr);
        png_infop info = png != nullptr ? png_create_info_struct(png) : nullptr;
        if (png == nullptr || info == nullptr) {
            if (png != nullptr) {
                png_destroy_read_struct(&png, nullptr, nullptr);
            }
            metadata["status"] = "backend_failed";
            metadata["decoded_by"] = "libpng";
            return metadata;
        }
        if (setjmp(png_jmpbuf(png)) != 0) {
            png_destroy_read_struct(&png, &info, nullptr);
            metadata["status"] = "invalid";
            metadata["decoded_by"] = "libpng";
            return metadata;
        }
        PngMemoryReader reader{bytes, 0};
        png_set_read_fn(png, &reader, &PngReadCallback);
        png_read_info(png, info);
        metadata["status"] = "decoded";
        metadata["format"] = "png";
        metadata["width"] = static_cast<Astra::Core::u32>(png_get_image_width(png, info));
        metadata["height"] = static_cast<Astra::Core::u32>(png_get_image_height(png, info));
        metadata["channels"] = static_cast<Astra::Core::u32>(png_get_channels(png, info));
        metadata["has_alpha"] = (png_get_color_type(png, info) & PNG_COLOR_MASK_ALPHA) != 0;
        metadata["decoded_by"] = "libpng";
        png_destroy_read_struct(&png, &info, nullptr);
        return metadata;
#else
        metadata["status"] = "backend_missing";
        metadata["decoded_by"] = "libpng";
        return metadata;
#endif
    }
    if (extension == ".jpg" || extension == ".jpeg") {
#if defined(ASTRA_ASSET_HAS_LIBJPEG)
        jpeg_decompress_struct jpeg{};
        JpegErrorHandler error{};
        jpeg.err = jpeg_std_error(&error.base);
        error.base.error_exit = &JpegErrorExit;
        if (setjmp(error.jump) != 0) {
            jpeg_destroy_decompress(&jpeg);
            metadata["status"] = "invalid";
            metadata["decoded_by"] = "libjpeg-turbo";
            return metadata;
        }
        jpeg_create_decompress(&jpeg);
        jpeg_mem_src(&jpeg, bytes.data(), static_cast<unsigned long>(bytes.size()));
        jpeg_read_header(&jpeg, TRUE);
        metadata["status"] = "decoded";
        metadata["format"] = "jpeg";
        metadata["width"] = static_cast<Astra::Core::u32>(jpeg.image_width);
        metadata["height"] = static_cast<Astra::Core::u32>(jpeg.image_height);
        metadata["channels"] = static_cast<Astra::Core::u32>(jpeg.num_components);
        metadata["has_alpha"] = false;
        metadata["decoded_by"] = "libjpeg-turbo";
        jpeg_destroy_decompress(&jpeg);
        return metadata;
#else
        metadata["status"] = "backend_missing";
        metadata["decoded_by"] = "libjpeg-turbo";
        return metadata;
#endif
    }
    if (extension == ".webp") {
#if defined(ASTRA_ASSET_HAS_LIBWEBP)
        int width = 0;
        int height = 0;
        if (WebPGetInfo(bytes.data(), bytes.size(), &width, &height) == 0) {
            metadata["status"] = "invalid";
            metadata["decoded_by"] = "libwebp";
            return metadata;
        }
        metadata["status"] = "decoded";
        metadata["format"] = "webp";
        metadata["width"] = width;
        metadata["height"] = height;
        metadata["channels"] = 4;
        metadata["has_alpha"] = true;
        metadata["decoded_by"] = "libwebp";
        return metadata;
#else
        metadata["status"] = "backend_missing";
        metadata["decoded_by"] = "libwebp";
        return metadata;
#endif
    }
    return metadata;
}

nlohmann::json InspectFontCookMetadata(std::span<const Astra::Core::u8> bytes) {
    nlohmann::json metadata = {
        {"schema", "astra.asset.font_cook_metadata.v1"},
        {"status", "backend_missing"},
        {"inspected_by", "FreeType"},
        {"shaped_by", "HarfBuzz"},
    };
#if defined(ASTRA_ASSET_HAS_FREETYPE)
    FT_Library library = nullptr;
    if (FT_Init_FreeType(&library) != 0) {
        metadata["status"] = "backend_failed";
        return metadata;
    }
    FT_Face face = nullptr;
    if (FT_New_Memory_Face(library, reinterpret_cast<const FT_Byte*>(bytes.data()), static_cast<FT_Long>(bytes.size()), 0, &face) != 0) {
        FT_Done_FreeType(library);
        metadata["status"] = "invalid";
        return metadata;
    }
    metadata["status"] = "inspected";
    metadata["family"] = face->family_name != nullptr ? face->family_name : "";
    metadata["style"] = face->style_name != nullptr ? face->style_name : "";
    metadata["glyph_count"] = static_cast<Astra::Core::u32>(face->num_glyphs);
    metadata["face_count"] = static_cast<Astra::Core::u32>(face->num_faces);
#if defined(ASTRA_ASSET_HAS_HARFBUZZ)
    hb_font_t* hb_font = hb_ft_font_create_referenced(face);
    hb_buffer_t* buffer = hb_buffer_create();
    constexpr std::string_view probe = "Astra";
    hb_buffer_add_utf8(buffer, probe.data(), static_cast<int>(probe.size()), 0, static_cast<int>(probe.size()));
    hb_buffer_guess_segment_properties(buffer);
    hb_shape(hb_font, buffer, nullptr, 0);
    unsigned int glyph_count = 0;
    (void)hb_buffer_get_glyph_infos(buffer, &glyph_count);
    metadata["shape_probe_glyphs"] = glyph_count;
    metadata["shaped_by"] = "HarfBuzz";
    hb_buffer_destroy(buffer);
    hb_font_destroy(hb_font);
#else
    metadata["shaped_by"] = "unavailable";
#endif
    FT_Done_Face(face);
    FT_Done_FreeType(library);
#endif
    return metadata;
}

nlohmann::json InspectAudioCookMetadata(std::span<const Astra::Core::u8> bytes, const std::filesystem::path& source_path) {
    nlohmann::json metadata = {
        {"schema", "astra.asset.audio_cook_metadata.v1"},
        {"status", "backend_missing"},
        {"format", LowerExtension(source_path)},
        {"decoded_by", "miniaudio"},
    };
#if defined(ASTRA_ASSET_HAS_MINIAUDIO)
    ma_decoder_config config = ma_decoder_config_init(ma_format_f32, 0, 0);
    ma_decoder decoder{};
    if (ma_decoder_init_memory(bytes.data(), bytes.size(), &config, &decoder) != MA_SUCCESS) {
        metadata["status"] = "invalid";
        return metadata;
    }
    ma_uint64 length = 0;
    (void)ma_decoder_get_length_in_pcm_frames(&decoder, &length);
    metadata["status"] = "decoded";
    metadata["channels"] = decoder.outputChannels;
    metadata["sample_rate"] = decoder.outputSampleRate;
    metadata["pcm_frame_count"] = static_cast<Astra::Core::u64>(length);
    ma_decoder_uninit(&decoder);
#endif
    return metadata;
}

std::string Sha256Text(std::string_view text) {
    EVP_MD_CTX* context = EVP_MD_CTX_new();
    EVP_DigestInit_ex(context, EVP_sha256(), nullptr);
    EVP_DigestUpdate(context, text.data(), text.size());
    std::array<unsigned char, EVP_MAX_MD_SIZE> digest{};
    unsigned int digest_size = 0;
    EVP_DigestFinal_ex(context, digest.data(), &digest_size);
    EVP_MD_CTX_free(context);

    std::ostringstream output;
    for (unsigned int index = 0; index < digest_size; ++index) {
        output << std::hex << std::setw(2) << std::setfill('0') << static_cast<int>(digest[index]);
    }
    return output.str();
}

Astra::Core::Result<std::vector<Astra::Core::u8>> DecodeBase64(std::string_view text) {
    if (text.empty()) {
        return Astra::Core::Result<std::vector<Astra::Core::u8>>::Success({});
    }
    if (text.size() > static_cast<std::size_t>(std::numeric_limits<int>::max())) {
        return Astra::Core::Result<std::vector<Astra::Core::u8>>::Failure(Astra::Core::ErrorCode::InvalidFormat, "base64 payload is too large");
    }
    std::vector<Astra::Core::u8> decoded(((text.size() + 3) / 4) * 3);
    const auto decoded_size = EVP_DecodeBlock(decoded.data(), reinterpret_cast<const unsigned char*>(text.data()), static_cast<int>(text.size()));
    if (decoded_size < 0) {
        return Astra::Core::Result<std::vector<Astra::Core::u8>>::Failure(Astra::Core::ErrorCode::InvalidFormat, "base64 payload is invalid");
    }
    auto size = static_cast<std::size_t>(decoded_size);
    if (!text.empty() && text.back() == '=') {
        --size;
    }
    if (text.size() >= 2 && text[text.size() - 2] == '=') {
        --size;
    }
    decoded.resize(size);
    return Astra::Core::Result<std::vector<Astra::Core::u8>>::Success(std::move(decoded));
}

std::string Sha256Bytes(const std::vector<Astra::Core::u8>& bytes) {
    EVP_MD_CTX* context = EVP_MD_CTX_new();
    EVP_DigestInit_ex(context, EVP_sha256(), nullptr);
    if (!bytes.empty()) {
        EVP_DigestUpdate(context, bytes.data(), bytes.size());
    }
    std::array<unsigned char, EVP_MAX_MD_SIZE> digest{};
    unsigned int digest_size = 0;
    EVP_DigestFinal_ex(context, digest.data(), &digest_size);
    EVP_MD_CTX_free(context);

    std::ostringstream output;
    for (unsigned int index = 0; index < digest_size; ++index) {
        output << std::hex << std::setw(2) << std::setfill('0') << static_cast<int>(digest[index]);
    }
    return output.str();
}

std::string Sha256Span(std::span<const Astra::Core::u8> bytes) {
    EVP_MD_CTX* context = EVP_MD_CTX_new();
    EVP_DigestInit_ex(context, EVP_sha256(), nullptr);
    if (!bytes.empty()) {
        EVP_DigestUpdate(context, bytes.data(), bytes.size());
    }
    std::array<unsigned char, EVP_MAX_MD_SIZE> digest{};
    unsigned int digest_size = 0;
    EVP_DigestFinal_ex(context, digest.data(), &digest_size);
    EVP_MD_CTX_free(context);

    std::ostringstream output;
    for (unsigned int index = 0; index < digest_size; ++index) {
        output << std::hex << std::setw(2) << std::setfill('0') << static_cast<int>(digest[index]);
    }
    return output.str();
}

std::string Sha256File(const std::filesystem::path& path) {
    return Sha256Span(ReadBytes(path));
}

std::vector<Astra::Core::u8> CompressZstd(std::span<const Astra::Core::u8> bytes) {
    std::vector<Astra::Core::u8> output(ZSTD_compressBound(bytes.size()));
    const auto size = ZSTD_compress(output.data(), output.size(), bytes.data(), bytes.size(), 3);
    if (ZSTD_isError(size)) {
        return {};
    }
    output.resize(size);
    return output;
}

Astra::Core::Result<std::vector<Astra::Core::u8>> DecompressZstd(std::span<const Astra::Core::u8> bytes, Astra::Core::u64 expected_size) {
    std::vector<Astra::Core::u8> output(static_cast<std::size_t>(expected_size));
    const auto size = ZSTD_decompress(output.data(), output.size(), bytes.data(), bytes.size());
    if (ZSTD_isError(size) || size != expected_size) {
        return Astra::Core::Result<std::vector<Astra::Core::u8>>::Failure(Astra::Core::ErrorCode::InvalidFormat, "could not decompress zstd package payload");
    }
    return Astra::Core::Result<std::vector<Astra::Core::u8>>::Success(std::move(output));
}

template <typename T>
void WritePod(std::ofstream& file, T value) {
    file.write(reinterpret_cast<const char*>(&value), sizeof(T));
}

template <typename T>
bool ReadPod(std::ifstream& file, T& value) {
    file.read(reinterpret_cast<char*>(&value), sizeof(T));
    return static_cast<bool>(file);
}

constexpr std::array<char, 8> kPackageMagic = {'A', 'S', 'T', 'R', 'A', 'P', '6', '\0'};
constexpr Astra::Core::u32 kPackageVersion = 1;

Astra::Core::u64 BinaryPackageHeaderSize() {
    return static_cast<Astra::Core::u64>(kPackageMagic.size() + sizeof(Astra::Core::u32) + sizeof(Astra::Core::u64));
}

Astra::Core::Result<nlohmann::json> ReadPackageJson(const std::filesystem::path& package_path, Astra::Core::DiagnosticSink& diagnostics) {
    std::ifstream file(package_path, std::ios::binary);
    if (!file) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_MANIFEST_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Package manifest could not be opened.", package_path));
        return Astra::Core::Result<nlohmann::json>::Failure(Astra::Core::ErrorCode::NotFound, "package manifest missing");
    }
    std::array<char, 8> magic{};
    file.read(magic.data(), static_cast<std::streamsize>(magic.size()));
    file.seekg(0);
    if (magic == kPackageMagic) {
        std::array<char, 8> read_magic{};
        Astra::Core::u32 version = 0;
        Astra::Core::u64 manifest_size = 0;
        file.read(read_magic.data(), static_cast<std::streamsize>(read_magic.size()));
        if (!ReadPod(file, version) || !ReadPod(file, manifest_size) || version != kPackageVersion || manifest_size == 0) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_HEADER_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Binary package header is invalid.", package_path));
            return Astra::Core::Result<nlohmann::json>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package header invalid");
        }
        std::string manifest_text(static_cast<std::size_t>(manifest_size), '\0');
        file.read(manifest_text.data(), static_cast<std::streamsize>(manifest_text.size()));
        if (!file) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_MANIFEST_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Binary package manifest is truncated.", package_path));
            return Astra::Core::Result<nlohmann::json>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package manifest truncated");
        }
        auto json = nlohmann::json::parse(manifest_text, nullptr, false);
        if (json.is_discarded()) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_MANIFEST_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Binary package manifest JSON is invalid.", package_path));
            return Astra::Core::Result<nlohmann::json>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package manifest invalid");
        }
        return Astra::Core::Result<nlohmann::json>::Success(std::move(json));
    }

    auto json = nlohmann::json::parse(file, nullptr, false);
    if (json.is_discarded()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_MANIFEST_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Package manifest JSON is invalid.", package_path));
        return Astra::Core::Result<nlohmann::json>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package manifest invalid");
    }
    return Astra::Core::Result<nlohmann::json>::Success(std::move(json));
}

nlohmann::json YamlToJson(const YAML::Node& node) {
    if (!node) {
        return nullptr;
    }
    if (node.IsScalar()) {
        return node.as<std::string>();
    }
    if (node.IsSequence()) {
        nlohmann::json array = nlohmann::json::array();
        for (const auto& item : node) {
            array.push_back(YamlToJson(item));
        }
        return array;
    }
    if (node.IsMap()) {
        nlohmann::json object = nlohmann::json::object();
        for (const auto& item : node) {
            object[item.first.as<std::string>()] = YamlToJson(item.second);
        }
        return object;
    }
    return nullptr;
}

std::vector<AssetUri> AssetUriArray(const nlohmann::json& json, std::string_view key, Astra::Core::DiagnosticSink& diagnostics, const std::filesystem::path& sidecar_path) {
    std::vector<AssetUri> uris;
    if (!json.contains(key)) {
        return uris;
    }
    const auto& values = json.at(key);
    if (!values.is_array()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_ASSET_DEPENDENCY_FORMAT", Astra::Core::DiagnosticSeverity::Blocking, "Asset dependency list must be an array.", sidecar_path));
        return uris;
    }
    for (const auto& value : values) {
        auto parsed = ParseAssetUri(value.get<std::string>());
        if (parsed) {
            uris.push_back(parsed.Value());
        } else {
            diagnostics.Emit(MakeDiagnostic("ASTRA_ASSET_DEPENDENCY_ID", Astra::Core::DiagnosticSeverity::Blocking, "Asset dependency has invalid id.", sidecar_path));
        }
    }
    return uris;
}

} // namespace

std::string AssetUri::ToString() const {
    std::string output = scheme_name + ":/" + NormalizePath(path);
    if (!fragment.empty()) {
        output += "#" + fragment;
    }
    return output;
}

Astra::Core::Result<void> ValidateImporterDescriptor(const ImporterDescriptor& descriptor, Astra::Core::DiagnosticSink& diagnostics) {
    bool valid = true;
    if (descriptor.provider_id.empty() || descriptor.contract != "IAssetImporter") {
        diagnostics.Emit(MakeDiagnostic("ASTRA_IMPORTER_DESCRIPTOR_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Importer descriptor requires provider_id and IAssetImporter contract."));
        valid = false;
    }
    if (descriptor.source_extensions.empty() || descriptor.output_asset_types.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_IMPORTER_DESCRIPTOR_FORMATS", Astra::Core::DiagnosticSeverity::Blocking, "Importer descriptor requires source extensions and output asset types."));
        valid = false;
    }
    if (descriptor.sidecar_schema != AssetSidecarSchema) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_IMPORTER_SIDECAR_SCHEMA", Astra::Core::DiagnosticSeverity::Blocking, "Importer descriptor uses unsupported sidecar schema."));
        valid = false;
    }
    return valid ? Astra::Core::Result<void>::Success() : Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "invalid importer descriptor");
}

Astra::Core::AssetId AssetUri::ToStableId() const {
    const auto parsed = Astra::Core::ParseStableId(ToString());
    if (parsed) {
        return parsed.Value();
    }
    return {};
}

std::string ToString(AssetScheme scheme) {
    switch (scheme) {
    case AssetScheme::Native:
        return "native";
    case AssetScheme::Virtual:
        return "virtual";
    case AssetScheme::Foreign:
        return "foreign";
    case AssetScheme::Package:
        return "package";
    case AssetScheme::Asset:
        return "asset";
    }
    return "asset";
}

AssetScheme AssetSchemeFromString(std::string_view value) {
    if (value == "native") {
        return AssetScheme::Native;
    }
    if (value == "virtual") {
        return AssetScheme::Virtual;
    }
    if (value == "package") {
        return AssetScheme::Package;
    }
    if (value.starts_with("foreign-")) {
        return AssetScheme::Foreign;
    }
    return AssetScheme::Asset;
}

Astra::Core::Result<AssetUri> ParseAssetUri(std::string_view text) {
    const auto split = text.find(":/");
    if (split == std::string_view::npos) {
        return Astra::Core::Result<AssetUri>::Failure(Astra::Core::ErrorCode::InvalidFormat, "asset uri must contain ':/'");
    }
    const auto scheme = text.substr(0, split);
    if (scheme != "asset" && scheme != "native" && scheme != "virtual" && scheme != "package" && !scheme.starts_with("foreign-")) {
        return Astra::Core::Result<AssetUri>::Failure(Astra::Core::ErrorCode::InvalidFormat, "asset uri has unsupported scheme");
    }
    auto rest = text.substr(split + 2);
    std::string fragment;
    const auto fragment_split = rest.find('#');
    if (fragment_split != std::string_view::npos) {
        fragment = std::string(rest.substr(fragment_split + 1));
        rest = rest.substr(0, fragment_split);
    }
    const auto normalized = NormalizePath(rest);
    if (normalized.empty() || normalized.find("..") != std::string::npos) {
        return Astra::Core::Result<AssetUri>::Failure(Astra::Core::ErrorCode::InvalidFormat, "asset uri path is empty or escapes its root");
    }
    return Astra::Core::Result<AssetUri>::Success({AssetSchemeFromString(scheme), std::string(scheme), normalized, fragment});
}

std::string DdcKey::ToString() const {
    nlohmann::json key = {
        {"schema", schema},
        {"asset_id", asset_id.ToString()},
        {"source_hash", source_hash},
        {"sidecar_hash", sidecar_hash},
        {"processor_id", processor_id},
        {"processor_version", processor_version},
        {"platform", platform},
        {"profile", profile},
        {"provider_feature_hash", provider_feature_hash},
    };
    return "ddc:/" + processor_id + "/" + Sha256Text(key.dump());
}

Astra::Core::Result<void> Vfs::Mount(VfsMount mount, Astra::Core::DiagnosticSink& diagnostics) {
    if (mount.mount_id.empty() || mount.scheme.empty() || mount.root.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_VFS_MOUNT_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "VFS mount requires mount id, scheme and root.", mount.root));
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidArgument, "invalid vfs mount");
    }
    mounts_.push_back(std::move(mount));
    std::ranges::sort(mounts_, [](const VfsMount& left, const VfsMount& right) {
        return left.priority > right.priority;
    });
    return Astra::Core::Result<void>::Success();
}

std::optional<ResolvedAssetPath> Vfs::Resolve(const AssetUri& uri) const {
    for (const auto& mount : mounts_) {
        if (mount.scheme == uri.scheme_name || (mount.scheme == "foreign" && uri.scheme == AssetScheme::Foreign)) {
            return ResolvedAssetPath{uri, mount, mount.root / uri.path, mount.access == VfsAccess::ReadOnly};
        }
    }
    return std::nullopt;
}

const std::vector<VfsMount>& Vfs::Mounts() const {
    return mounts_;
}

Astra::Core::Result<AssetSidecar> AssetSidecarFromJson(const nlohmann::json& json, const std::filesystem::path& sidecar_path, Astra::Core::DiagnosticSink& diagnostics) {
    if (!json.contains("id")) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_ASSET_SIDECAR_REQUIRED", Astra::Core::DiagnosticSeverity::Blocking, "Asset sidecar requires id.", sidecar_path));
        return Astra::Core::Result<AssetSidecar>::Failure(Astra::Core::ErrorCode::InvalidFormat, "sidecar missing required fields");
    }
    auto id = ParseAssetUri(json.at("id").get<std::string>());
    if (!id) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_ASSET_ID_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Asset sidecar id is invalid.", sidecar_path));
        return Astra::Core::Result<AssetSidecar>::Failure(id.Error(), id.Message());
    }
    AssetSidecar sidecar;
    sidecar.schema = json.value("schema", AssetSidecarSchema);
    sidecar.id = id.Value();
    sidecar.type = json.value("type", "");
    sidecar.source_path = json.value("source_path", "");
    sidecar.display_name = json.value("display_name", "");
    sidecar.tags = json.value("tags", std::vector<std::string>{});
    sidecar.origin = json.value("origin", "HumanAuthored");
    if (json.contains("license")) {
        sidecar.license.owner = json.at("license").value("owner", "");
        sidecar.license.usage = json.at("license").value("usage", "");
        sidecar.license.source = json.at("license").value("source", "");
    }
    if (json.contains("review")) {
        sidecar.review.status = json.at("review").value("status", "accepted");
        sidecar.review.review_item = json.at("review").value("review_item", "");
    }
    if (json.contains("cook")) {
        sidecar.cook.preset = json.at("cook").value("preset", json.at("cook").value("texture_preset", ""));
        sidecar.cook.settings = json.at("cook");
    }
    if (json.contains("dependencies")) {
        sidecar.hard_dependencies = AssetUriArray(json.at("dependencies"), "hard", diagnostics, sidecar_path);
        sidecar.soft_dependencies = AssetUriArray(json.at("dependencies"), "soft", diagnostics, sidecar_path);
    }
    sidecar.requires_review = json.value("requires_review", false);
    if (json.contains("ai_generation")) {
        AiGenerationInfo info;
        info.provider = json.at("ai_generation").value("provider", "");
        info.session_hash = json.at("ai_generation").value("session_hash", "");
        info.prompt_hash = json.at("ai_generation").value("prompt_hash", "");
        info.context_hash = json.at("ai_generation").value("context_hash", "");
        info.output_hash = json.at("ai_generation").value("output_hash", "");
        info.source_draft = json.at("ai_generation").value("source_draft", "");
        sidecar.ai_generation = std::move(info);
    }
    return Astra::Core::Result<AssetSidecar>::Success(std::move(sidecar));
}

Astra::Core::Result<AssetSidecar> LoadAssetSidecar(const std::filesystem::path& path, Astra::Core::DiagnosticSink& diagnostics) {
    try {
        const auto json = YamlToJson(YAML::LoadFile(path.string()));
        auto sidecar = AssetSidecarFromJson(json, path, diagnostics);
        if (!sidecar) {
            return sidecar;
        }
        auto validation = ValidateAssetSidecar(sidecar.Value(), path, diagnostics);
        if (!validation) {
            return Astra::Core::Result<AssetSidecar>::Failure(validation.Error(), validation.Message());
        }
        return sidecar;
    } catch (const YAML::Exception& error) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_ASSET_SIDECAR_PARSE", Astra::Core::DiagnosticSeverity::Blocking, error.what(), path));
        return Astra::Core::Result<AssetSidecar>::Failure(Astra::Core::ErrorCode::InvalidFormat, error.what());
    }
}

Astra::Core::Result<void> ValidateAssetSidecar(const AssetSidecar& sidecar, const std::filesystem::path& sidecar_path, Astra::Core::DiagnosticSink& diagnostics) {
    bool valid = true;
    if (sidecar.id.scheme == AssetScheme::Virtual || sidecar.id.scheme == AssetScheme::Package) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_ASSET_SIDECAR_SCHEME", Astra::Core::DiagnosticSeverity::Blocking, "Source sidecars must use native:/ or foreign-*:/ ids.", sidecar_path));
        valid = false;
    }
    if (sidecar.type.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_ASSET_TYPE_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Asset sidecar type is missing.", sidecar_path));
        valid = false;
    }
    if (sidecar.source_path.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_ASSET_SOURCE_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Asset sidecar source_path is missing.", sidecar_path));
        valid = false;
    }
    if (sidecar.license.owner.empty() || sidecar.license.usage.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_ASSET_LICENSE_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Asset sidecar license owner and usage are required.", sidecar_path));
        valid = false;
    }
    if ((sidecar.requires_review || sidecar.origin == "AIGenerated" || sidecar.ai_generation.has_value()) && sidecar.review.status != "accepted") {
        diagnostics.Emit(MakeDiagnostic("ASTRA_RELEASE_ASSET_004", Astra::Core::DiagnosticSeverity::Blocking, "Unreviewed AI asset cannot enter foundation registry.", sidecar_path));
        valid = false;
    }
    return valid ? Astra::Core::Result<void>::Success() : Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "asset sidecar validation failed");
}

AssetRegistry AssetRegistryBuilder::Scan(const std::filesystem::path& content_root, Astra::Core::DiagnosticSink& diagnostics) const {
    AssetRegistry registry;
    std::set<std::string> ids;
    if (!std::filesystem::exists(content_root)) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_ASSET_CONTENT_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Content root does not exist.", content_root));
        return registry;
    }
    for (const auto& entry : std::filesystem::recursive_directory_iterator(content_root)) {
        if (!entry.is_regular_file() || entry.path().filename().string().find(".asset.yaml") == std::string::npos) {
            continue;
        }
        Astra::Core::Result<AssetSidecar> sidecar =
            Astra::Core::Result<AssetSidecar>::Failure(Astra::Core::ErrorCode::InvalidFormat, "asset sidecar not loaded");
        const auto diagnostic_start = diagnostics.Diagnostics().size();
        try {
            const auto json = YamlToJson(YAML::LoadFile(entry.path().string()));
            sidecar = AssetSidecarFromJson(json, entry.path(), diagnostics);
        } catch (const YAML::Exception& error) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_ASSET_SIDECAR_PARSE", Astra::Core::DiagnosticSeverity::Blocking, error.what(), entry.path()));
        }
        if (!sidecar) {
            continue;
        }
        AssetRegistryEntry registry_entry;
        (void)ValidateAssetSidecar(sidecar.Value(), entry.path(), diagnostics);
        registry_entry.id = sidecar.Value().id;
        registry_entry.type = sidecar.Value().type;
        registry_entry.sidecar_path = entry.path();
        registry_entry.source_path = entry.path().parent_path() / sidecar.Value().source_path;
        registry_entry.sidecar_hash = Sha256Text(ReadText(entry.path()));
        registry_entry.origin = sidecar.Value().origin;
        registry_entry.license = sidecar.Value().license;
        registry_entry.review = sidecar.Value().review;
        registry_entry.requires_review = sidecar.Value().requires_review;
        registry_entry.has_ai_generation = sidecar.Value().ai_generation.has_value();
        const auto& validation_diagnostics = diagnostics.Diagnostics();
        for (std::size_t index = diagnostic_start; index < validation_diagnostics.size(); ++index) {
            registry_entry.diagnostics.push_back(validation_diagnostics[index]);
        }
        if (!sidecar.Value().source_path.empty() && std::filesystem::is_regular_file(registry_entry.source_path)) {
            registry_entry.source_hash = Sha256File(registry_entry.source_path);
        } else {
            auto diagnostic = MakeDiagnostic("ASTRA_ASSET_SOURCE_NOT_FOUND", Astra::Core::DiagnosticSeverity::Blocking, "Asset source file is missing.", registry_entry.source_path);
            diagnostic.objects.push_back({"AssetId", registry_entry.id.ToString()});
            registry_entry.diagnostics.push_back(diagnostic);
            diagnostics.Emit(diagnostic);
        }
        if (!ids.insert(registry_entry.id.ToString()).second) {
            auto diagnostic = MakeDiagnostic("ASTRA_ASSET_ID_DUPLICATE", Astra::Core::DiagnosticSeverity::Blocking, "Duplicate AssetId found in sidecar registry scan.", entry.path());
            diagnostic.objects.push_back({"AssetId", registry_entry.id.ToString()});
            registry_entry.diagnostics.push_back(diagnostic);
            diagnostics.Emit(diagnostic);
        }
        registry_entry.hard_dependencies = sidecar.Value().hard_dependencies;
        registry_entry.soft_dependencies = sidecar.Value().soft_dependencies;
        registry.entries.push_back(std::move(registry_entry));
    }
    std::set<std::string> known;
    for (const auto& entry : registry.entries) {
        known.insert(entry.id.ToString());
    }
    for (auto& entry : registry.entries) {
        for (const auto& dependency : entry.hard_dependencies) {
            if (!known.contains(dependency.ToString())) {
                auto diagnostic = MakeDiagnostic("ASTRA_ASSET_DEPENDENCY_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Hard asset dependency is missing.", entry.sidecar_path);
                diagnostic.objects.push_back({"AssetId", entry.id.ToString()});
                diagnostic.objects.push_back({"MissingAssetId", dependency.ToString()});
                entry.diagnostics.push_back(diagnostic);
                diagnostics.Emit(std::move(diagnostic));
            }
        }
    }
    return registry;
}

std::vector<AssetInvalidation> AssetRegistryBuilder::WatchContent(Astra::Platform::IFileSystemService& filesystem, const std::filesystem::path& content_root) const {
    std::vector<AssetInvalidation> invalidations;
    filesystem.Watch(content_root, [&](const std::filesystem::path& changed) {
        invalidations.push_back({changed, {}, "platform_watch"});
    });
    return invalidations;
}

std::vector<AssetInvalidation> AssetRegistryBuilder::ComputeInvalidations(const AssetRegistry& before, const AssetRegistry& after) const {
    std::map<std::string, AssetRegistryEntry> previous;
    for (const auto& entry : before.entries) {
        previous[entry.id.ToString()] = entry;
    }
    std::vector<AssetInvalidation> invalidations;
    for (const auto& entry : after.entries) {
        const auto id = entry.id.ToString();
        const auto found = previous.find(id);
        if (found == previous.end()) {
            invalidations.push_back({entry.source_path, {entry.id}, "asset_added"});
            continue;
        }
        if (found->second.source_hash != entry.source_hash || found->second.sidecar_hash != entry.sidecar_hash) {
            invalidations.push_back({entry.source_path, {entry.id}, "asset_changed"});
        }
    }
    std::set<std::string> after_ids;
    for (const auto& entry : after.entries) {
        after_ids.insert(entry.id.ToString());
    }
    for (const auto& entry : before.entries) {
        if (!after_ids.contains(entry.id.ToString())) {
            invalidations.push_back({entry.source_path, {entry.id}, "asset_removed"});
        }
    }
    return invalidations;
}

Astra::Core::Result<void> ValidateImportPreset(const ImportPresetDescriptor& descriptor, Astra::Core::DiagnosticSink& diagnostics) {
    if (descriptor.id.empty() || descriptor.source_extensions.empty() || descriptor.asset_type.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_IMPORT_PRESET_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Import preset requires id, source extensions and asset type."));
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "invalid import preset");
    }
    return Astra::Core::Result<void>::Success();
}

Astra::Core::Result<void> ValidateImportRequest(const ImportRequest& request, Astra::Core::DiagnosticSink& diagnostics) {
    bool valid = true;
    if (request.schema != ImportRequestSchema) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_IMPORT_REQUEST_SCHEMA", Astra::Core::DiagnosticSeverity::Blocking, "Import request schema is unsupported.", request.source_path));
        valid = false;
    }
    if (request.source_path.empty() || !std::filesystem::exists(request.source_path)) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_IMPORT_SOURCE_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Import source file is missing.", request.source_path));
        valid = false;
    }
    if (request.target_asset_id.scheme != AssetScheme::Native && request.target_asset_id.scheme != AssetScheme::Foreign) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_IMPORT_TARGET_SCHEME", Astra::Core::DiagnosticSeverity::Blocking, "Import target must be native:/ or foreign-*:/.", request.source_path));
        valid = false;
    }
    if (request.asset_type.empty() || request.preset.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_IMPORT_REQUEST_REQUIRED", Astra::Core::DiagnosticSeverity::Blocking, "Import request requires asset_type and preset.", request.source_path));
        valid = false;
    }
    if (request.license.owner.empty() || request.license.usage.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_IMPORT_LICENSE_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Import request requires license owner and usage.", request.source_path));
        valid = false;
    }
    if ((request.origin == "AIGenerated" || request.review_state != "accepted") && request.review_state != "accepted") {
        diagnostics.Emit(MakeDiagnostic("ASTRA_RELEASE_ASSET_004", Astra::Core::DiagnosticSeverity::Blocking, "Unreviewed AI asset cannot be imported.", request.source_path));
        valid = false;
    }
    if (request.target_asset_id.scheme == AssetScheme::Foreign && request.copy_source && !request.foreign_copy_allowed) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_RELEASE_ASSET_FOREIGN_COPY", Astra::Core::DiagnosticSeverity::Blocking, "Foreign assets are mount-only unless foreign copy is explicitly allowed.", request.source_path));
        valid = false;
    }
    return valid ? Astra::Core::Result<void>::Success() : Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "invalid import request");
}

Astra::Core::Result<void> ValidateProjectTemplate(const ProjectTemplateDescriptor& descriptor, Astra::Core::DiagnosticSink& diagnostics) {
    if (descriptor.id.empty() || descriptor.display_name.empty() || descriptor.runtime_profile.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PROJECT_TEMPLATE_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Project template requires id, display name and runtime profile."));
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "invalid project template");
    }
    return Astra::Core::Result<void>::Success();
}

Astra::Core::Result<void> ValidateReviewQueueItem(const ReviewQueueItem& item, Astra::Core::DiagnosticSink& diagnostics) {
    if (item.id.empty() || item.kind.empty() || item.source_ref.empty() || item.target_ref.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_REVIEW_ITEM_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Review item requires id, kind, source ref and target ref."));
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "invalid review queue item");
    }
    return Astra::Core::Result<void>::Success();
}

namespace {

ImporterDescriptor DescriptorForImporter(std::string_view provider_id) {
    if (provider_id == "astra.importer.image") {
        return {"astra.importer.image", "IAssetImporter", {".png", ".jpg", ".jpeg", ".webp"}, {"image", "image.sprite", "image.background"}, AssetSidecarSchema, "ASTRA_IMPORT_IMAGE"};
    }
    if (provider_id == "astra.importer.audio") {
        return {"astra.importer.audio", "IAssetImporter", {".ogg", ".wav", ".flac", ".mp3"}, {"audio"}, AssetSidecarSchema, "ASTRA_IMPORT_AUDIO"};
    }
    if (provider_id == "astra.importer.font") {
        return {"astra.importer.font", "IAssetImporter", {".otf", ".ttf"}, {"font"}, AssetSidecarSchema, "ASTRA_IMPORT_FONT"};
    }
    if (provider_id == "astra.importer.text") {
        return {"astra.importer.text", "IAssetImporter", {".txt", ".yaml", ".json"}, {"text", "timeline"}, AssetSidecarSchema, "ASTRA_IMPORT_TEXT"};
    }
    if (provider_id == "astra.importer.filter_profile") {
        return {"astra.importer.filter_profile", "IAssetImporter", {".filter.yaml"}, {"filter_profile"}, AssetSidecarSchema, "ASTRA_IMPORT_FILTER"};
    }
    if (provider_id == "astra.importer.script") {
        return {"astra.importer.script", "IAssetImporter", {".astra", ".lua"}, {"script.native", "script.lua"}, AssetSidecarSchema, "ASTRA_IMPORT_SCRIPT"};
    }
    return {};
}

class GenericAssetImporter final : public IAssetImporter {
public:
    explicit GenericAssetImporter(ImporterDescriptor descriptor) : descriptor_(std::move(descriptor)) {}

    [[nodiscard]] ImporterDescriptor Describe() const override { return descriptor_; }

    [[nodiscard]] Astra::Core::Result<ImportPreview> Preview(const ImportRequest& request, Astra::Core::DiagnosticSink& diagnostics) const override {
        auto validation = ValidateImportRequest(request, diagnostics);
        if (!validation) {
            return Astra::Core::Result<ImportPreview>::Failure(validation.Error(), validation.Message());
        }
        const auto extension = LowerExtension(request.source_path);
        if (std::ranges::find(descriptor_.source_extensions, extension) == descriptor_.source_extensions.end()) {
            diagnostics.Emit(MakeDiagnostic(descriptor_.diagnostics_prefix + std::string("_EXTENSION_UNSUPPORTED"), Astra::Core::DiagnosticSeverity::Blocking, "Importer does not support the source extension.", request.source_path));
            return Astra::Core::Result<ImportPreview>::Failure(Astra::Core::ErrorCode::Unsupported, "unsupported import extension");
        }
        ImportPreview preview;
        preview.importer = descriptor_;
        preview.request = request;
        preview.metadata = {
            {"source_extension", extension},
            {"size_bytes", static_cast<Astra::Core::u64>(std::filesystem::file_size(request.source_path))},
            {"source_hash", Sha256File(request.source_path)},
        };
        return Astra::Core::Result<ImportPreview>::Success(std::move(preview));
    }

    [[nodiscard]] Astra::Core::Result<ImportedAsset> Import(const ImportRequest& request, const std::filesystem::path& content_root, Astra::Core::DiagnosticSink& diagnostics) const override {
        auto preview = Preview(request, diagnostics);
        if (!preview) {
            return Astra::Core::Result<ImportedAsset>::Failure(preview.Error(), preview.Message());
        }
        const auto relative_asset_path = std::filesystem::path(NormalizePath(request.target_asset_id.path));
        const auto target_source = content_root / relative_asset_path.parent_path() / request.source_path.filename();
        const auto sidecar_path = target_source.parent_path() / (target_source.stem().string() + ".asset.yaml");
        if (request.copy_source) {
            std::filesystem::create_directories(target_source.parent_path());
            std::filesystem::copy_file(request.source_path, target_source, std::filesystem::copy_options::overwrite_existing);
        }
        AssetSidecar sidecar;
        sidecar.id = request.target_asset_id;
        sidecar.type = request.asset_type;
        sidecar.source_path = target_source.filename();
        sidecar.display_name = target_source.stem().string();
        sidecar.origin = request.origin == "user_imported" ? "HumanAuthored" : request.origin;
        sidecar.license = request.license;
        sidecar.review.status = request.review_state;
        sidecar.cook.preset = request.preset;

        nlohmann::json json = ToJson(sidecar);
        std::ofstream out(sidecar_path, std::ios::binary);
        out << "schema: " << json["schema"].get<std::string>() << "\n";
        out << "id: " << sidecar.id.ToString() << "\n";
        out << "type: " << sidecar.type << "\n";
        out << "source_path: " << sidecar.source_path.generic_string() << "\n";
        out << "display_name: " << sidecar.display_name << "\n";
        out << "origin: " << sidecar.origin << "\n";
        out << "license:\n  owner: " << sidecar.license.owner << "\n  usage: " << sidecar.license.usage << "\n";
        out << "review:\n  status: " << sidecar.review.status << "\n";
        out << "cook:\n  preset: " << sidecar.cook.preset << "\n";
        out << "dependencies:\n  hard: []\n  soft: []\n";

        ImportedAsset imported;
        imported.sidecar = std::move(sidecar);
        imported.sidecar_path = sidecar_path;
        imported.source_path = target_source;
        imported.audit = {{"schema", "astra.asset.import_audit.v1"}, {"importer", descriptor_.provider_id}, {"source_hash", preview.Value().metadata["source_hash"]}};
        return Astra::Core::Result<ImportedAsset>::Success(std::move(imported));
    }

private:
    ImporterDescriptor descriptor_;
};

CookProcessorDescriptor DescriptorForProcessor(std::string_view provider_id) {
    if (provider_id == "astra.cook.image.texture") {
        return {"astra.cook.image.texture", "ICookProcessor", {"image", "image.sprite", "image.background"}, {"astra.texture.source", "astra.texture.rgba8"}, {"astra.image_decode"}, true, 1, "ASTRA_COOK_IMAGE"};
    }
    if (provider_id == "astra.cook.audio.stream") {
        return {"astra.cook.audio.stream", "ICookProcessor", {"audio"}, {"astra.audio.stream"}, {"astra.audio_decode"}, true, 1, "ASTRA_COOK_AUDIO"};
    }
    if (provider_id == "astra.cook.font.atlas") {
        return {"astra.cook.font.atlas", "ICookProcessor", {"font"}, {"astra.font.runtime"}, {"astra.text_layout"}, true, 1, "ASTRA_COOK_FONT"};
    }
    if (provider_id == "astra.cook.filter_profile") {
        return {"astra.cook.filter_profile", "ICookProcessor", {"filter_profile"}, {"astra.filter_profile.runtime"}, {}, true, 1, "ASTRA_COOK_FILTER"};
    }
    if (provider_id == "astra.cook.script.native") {
        return {"astra.cook.script.native", "ICookProcessor", {"script.native"}, {"astra.script.native.ir"}, {"astra.script.native"}, true, 1, "ASTRA_COOK_SCRIPT"};
    }
    if (provider_id == "astra.cook.script.lua") {
        return {"astra.cook.script.lua", "ICookProcessor", {"script.lua"}, {"astra.script.lua.bytecode"}, {"astra.script.lua"}, true, 1, "ASTRA_COOK_SCRIPT"};
    }
    if (provider_id == "astra.cook.timeline") {
        return {"astra.cook.timeline", "ICookProcessor", {"timeline", "text"}, {"astra.timeline.runtime"}, {}, true, 1, "ASTRA_COOK_TIMELINE"};
    }
    return {"astra.cook.asset.binary", "ICookProcessor", {"asset"}, {"astra.asset.binary"}, {}, true, 1, "ASTRA_COOK_ASSET"};
}

std::string ProcessorIdForType(std::string_view type) {
    if (type == "image" || type == "image.sprite" || type == "image.background") {
        return "astra.cook.image.texture";
    }
    if (type == "audio") {
        return "astra.cook.audio.stream";
    }
    if (type == "font") {
        return "astra.cook.font.atlas";
    }
    if (type == "filter_profile") {
        return "astra.cook.filter_profile";
    }
    if (type == "script.native") {
        return "astra.cook.script.native";
    }
    if (type == "script.lua") {
        return "astra.cook.script.lua";
    }
    if (type == "timeline" || type == "text") {
        return "astra.cook.timeline";
    }
    return "astra.cook.asset.binary";
}

class GenericCookProcessor final : public ICookProcessor {
public:
    explicit GenericCookProcessor(CookProcessorDescriptor descriptor) : descriptor_(std::move(descriptor)) {}

    [[nodiscard]] CookProcessorDescriptor Describe() const override { return descriptor_; }

    [[nodiscard]] Astra::Core::Result<CookArtifactDescriptor> Cook(const CookRequest& request, std::span<const Astra::Core::u8> source_bytes, Astra::Core::DiagnosticSink& diagnostics) const override {
        if (!descriptor_.package_eligible) {
            diagnostics.Emit(MakeDiagnostic(descriptor_.diagnostics_prefix + std::string("_NOT_PACKAGE_ELIGIBLE"), Astra::Core::DiagnosticSeverity::Blocking, "Cook processor is not package eligible.", request.asset.source_path));
            return Astra::Core::Result<CookArtifactDescriptor>::Failure(Astra::Core::ErrorCode::PermissionDenied, "processor not package eligible");
        }
        if (std::ranges::find(descriptor_.input_asset_types, request.asset.type) == descriptor_.input_asset_types.end() && descriptor_.provider_id != "astra.cook.asset.binary") {
            diagnostics.Emit(MakeDiagnostic(descriptor_.diagnostics_prefix + std::string("_INPUT_UNSUPPORTED"), Astra::Core::DiagnosticSeverity::Blocking, "Cook processor does not support the asset type.", request.asset.source_path));
            return Astra::Core::Result<CookArtifactDescriptor>::Failure(Astra::Core::ErrorCode::Unsupported, "unsupported cook input type");
        }
        CookArtifactDescriptor artifact;
        artifact.asset_id = request.asset.id;
        artifact.artifact_id = "cooked:/" + NormalizePath(request.asset.id.path) + ".bin";
        artifact.format = descriptor_.output_formats.empty() ? "astra.asset.binary" : descriptor_.output_formats.front();
        DdcKey key;
        key.asset_id = request.asset.id;
        key.source_hash = request.asset.source_hash;
        key.sidecar_hash = request.asset.sidecar_hash;
        key.processor_id = descriptor_.provider_id;
        key.processor_version = descriptor_.version;
        key.platform = request.target_platform;
        key.profile = request.release_profile;
        key.provider_feature_hash = request.provider_feature_hash;
        artifact.ddc_key = key.ToString();
        artifact.payload_hash = Sha256Span(source_bytes);
        artifact.payload_size = static_cast<Astra::Core::u64>(source_bytes.size());
        artifact.runtime_dependencies = request.asset.hard_dependencies;
        artifact.metadata = {
            {"processor", descriptor_.provider_id},
            {"processor_version", descriptor_.version},
            {"source_extension", LowerExtension(request.asset.source_path)},
            {"provider_feature_hash", request.provider_feature_hash},
            {"package_eligible", descriptor_.package_eligible},
        };
        if (request.asset.type == "image" || request.asset.type == "image.sprite" || request.asset.type == "image.background") {
            artifact.metadata["media_inspect"] = InspectImageCookMetadata(source_bytes, request.asset.source_path);
        } else if (request.asset.type == "font") {
            artifact.metadata["font_inspect"] = InspectFontCookMetadata(source_bytes);
        } else if (request.asset.type == "audio") {
            artifact.metadata["audio_inspect"] = InspectAudioCookMetadata(source_bytes, request.asset.source_path);
        }
        return Astra::Core::Result<CookArtifactDescriptor>::Success(std::move(artifact));
    }

private:
    CookProcessorDescriptor descriptor_;
};

std::filesystem::path CookedPathForAsset(const CookPipelineOptions& options, const AssetUri& asset) {
    return options.cooked_root / NormalizePath(asset.path + ".astbin");
}

std::filesystem::path DdcPathForKey(const std::filesystem::path& ddc_root, std::string_view key) {
    auto safe = std::string(key);
    std::replace(safe.begin(), safe.end(), ':', '_');
    std::replace(safe.begin(), safe.end(), '/', '_');
    return ddc_root / "cache" / (safe + ".bin");
}

bool IsSupportedCookArtifact(std::string_view processor_id, const nlohmann::json& metadata) {
    const auto processors = BuiltinCookProcessorDescriptors();
    const auto found = std::ranges::find_if(processors, [&](const CookProcessorDescriptor& descriptor) {
        return descriptor.provider_id == processor_id;
    });
    if (found == processors.end() || !found->package_eligible) {
        return false;
    }
    const auto format = metadata.value("format", std::string());
    return !format.empty() &&
           std::ranges::find(found->output_formats, format) != found->output_formats.end();
}

} // namespace

std::vector<ImporterDescriptor> BuiltinImporterDescriptors() {
    return {
        DescriptorForImporter("astra.importer.image"),
        DescriptorForImporter("astra.importer.audio"),
        DescriptorForImporter("astra.importer.font"),
        DescriptorForImporter("astra.importer.text"),
        DescriptorForImporter("astra.importer.filter_profile"),
        DescriptorForImporter("astra.importer.script"),
    };
}

std::vector<CookProcessorDescriptor> BuiltinCookProcessorDescriptors() {
    return {
        DescriptorForProcessor("astra.cook.image.texture"),
        DescriptorForProcessor("astra.cook.audio.stream"),
        DescriptorForProcessor("astra.cook.font.atlas"),
        DescriptorForProcessor("astra.cook.filter_profile"),
        DescriptorForProcessor("astra.cook.script.native"),
        DescriptorForProcessor("astra.cook.script.lua"),
        DescriptorForProcessor("astra.cook.timeline"),
        DescriptorForProcessor("astra.cook.asset.binary"),
    };
}

std::unique_ptr<IAssetImporter> CreateBuiltinImporter(std::string_view provider_id) {
    auto descriptor = DescriptorForImporter(provider_id);
    if (descriptor.provider_id.empty()) {
        return {};
    }
    return std::make_unique<GenericAssetImporter>(std::move(descriptor));
}

std::unique_ptr<ICookProcessor> CreateBuiltinCookProcessor(std::string_view provider_id) {
    auto descriptor = DescriptorForProcessor(provider_id);
    if (descriptor.provider_id.empty()) {
        return {};
    }
    return std::make_unique<GenericCookProcessor>(std::move(descriptor));
}

Astra::Core::Result<CookManifest> CookAssetRegistry(const AssetRegistry& registry, const CookPipelineOptions& options, Astra::Core::DiagnosticSink& diagnostics) {
    CookManifest manifest;
    manifest.project_id = options.project_id.empty() ? "package:/project" : options.project_id;
    manifest.profile = options.profile;
    std::filesystem::create_directories(options.cooked_root);
    std::filesystem::create_directories(options.ddc_root);

    for (const auto& entry : registry.entries) {
        if (!entry.diagnostics.empty()) {
            for (const auto& diagnostic : entry.diagnostics) {
                if (diagnostic.BlocksRelease()) {
                    diagnostics.Emit(diagnostic);
                    return Astra::Core::Result<CookManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "registry contains blocking diagnostics");
                }
            }
        }
        if (!std::filesystem::exists(entry.source_path)) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_COOK_SOURCE_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Cook source asset is missing.", entry.source_path));
            return Astra::Core::Result<CookManifest>::Failure(Astra::Core::ErrorCode::NotFound, "cook source missing");
        }
        const auto processor_id = ProcessorIdForType(entry.type);
        auto processor = CreateBuiltinCookProcessor(processor_id);
        if (!processor) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_COOK_PROCESSOR_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "No cook processor is registered for the asset type.", entry.source_path));
            return Astra::Core::Result<CookManifest>::Failure(Astra::Core::ErrorCode::Unsupported, "cook processor missing");
        }
        CookRequest request;
        request.asset = entry;
        request.target_platform = options.target_platform;
        request.release_profile = options.profile;
        request.selected_providers = options.selected_providers;
        request.provider_feature_hash = options.provider_feature_hash.empty() ? ComputeProviderFeatureHash(options.selected_providers) : options.provider_feature_hash;

        const auto source_bytes = ReadBytes(entry.source_path);
        auto cooked = processor->Cook(request, source_bytes, diagnostics);
        if (!cooked) {
            return Astra::Core::Result<CookManifest>::Failure(cooked.Error(), cooked.Message());
        }

        const auto cooked_path = CookedPathForAsset(options, entry.id);
        const auto ddc_path = DdcPathForKey(options.ddc_root, cooked.Value().ddc_key);
        const auto existing_hash = std::filesystem::exists(ddc_path) ? Sha256File(ddc_path) : std::string();
        const bool corruption = !existing_hash.empty() && existing_hash != cooked.Value().payload_hash;
        const bool rebuild = existing_hash != cooked.Value().payload_hash;
        if (rebuild) {
            WriteBytes(ddc_path, source_bytes);
        }
        WriteBytes(cooked_path, source_bytes);

        CookArtifact artifact;
        artifact.asset_id = entry.id;
        artifact.type = entry.type;
        artifact.artifact_path = cooked_path;
        artifact.hash = cooked.Value().payload_hash;
        artifact.processor_id = processor_id;
        artifact.ddc_key = cooked.Value().ddc_key;
        artifact.metadata = cooked.Value().metadata;
        artifact.metadata["artifact_id"] = cooked.Value().artifact_id;
        artifact.metadata["format"] = cooked.Value().format;
        artifact.metadata["runtime_dependencies"] = nlohmann::json::array();
        for (const auto& dependency : cooked.Value().runtime_dependencies) {
            artifact.metadata["runtime_dependencies"].push_back(dependency.ToString());
        }
        manifest.artifacts.push_back(std::move(artifact));

        DerivedDataCacheEntry ddc;
        ddc.key = cooked.Value().ddc_key;
        ddc.processor_id = processor_id;
        ddc.input_hash = entry.source_hash;
        ddc.settings_hash = Sha256Text(processor_id + "|" + entry.sidecar_hash + "|" + options.profile + "|" + request.provider_feature_hash);
        ddc.output_hash = cooked.Value().payload_hash;
        ddc.platform = options.target_platform;
        ddc.profile = options.profile;
        ddc.engine_version = "0.2.0";
        ddc.rebuilt = rebuild;
        ddc.corruption_recovered = corruption;
        ddc.artifacts.push_back({ddc_path, cooked.Value().payload_hash, static_cast<Astra::Core::u64>(source_bytes.size())});
        manifest.ddc_entries.push_back(std::move(ddc));
    }
    manifest.manifest_hash = ComputeCookManifestHash(ToJson(manifest));
    return Astra::Core::Result<CookManifest>::Success(std::move(manifest));
}

DdcCleanReport CleanDerivedDataCache(const std::filesystem::path& ddc_root, const std::vector<DerivedDataCacheEntry>& live_entries, Astra::Core::DiagnosticSink& diagnostics) {
    DdcCleanReport report;
    report.root = ddc_root;
    const auto cache_root = ddc_root / "cache";
    if (!std::filesystem::exists(cache_root)) {
        return report;
    }

    std::set<std::filesystem::path> retained_paths;
    for (const auto& entry : live_entries) {
        for (const auto& artifact : entry.artifacts) {
            retained_paths.insert(std::filesystem::absolute(artifact.path).lexically_normal());
            retained_paths.insert(std::filesystem::absolute(ddc_root / artifact.path).lexically_normal());
        }
        if (!entry.key.empty()) {
            retained_paths.insert(std::filesystem::absolute(DdcPathForKey(ddc_root, entry.key)).lexically_normal());
        }
    }

    const auto absolute_cache = std::filesystem::absolute(cache_root).lexically_normal();
    for (const auto& item : std::filesystem::recursive_directory_iterator(cache_root)) {
        if (!item.is_regular_file()) {
            continue;
        }
        ++report.scanned;
        const auto candidate = std::filesystem::absolute(item.path()).lexically_normal();
        if (!candidate.generic_string().starts_with(absolute_cache.generic_string())) {
            auto diagnostic = MakeDiagnostic("ASTRA_DDC_CLEAN_PATH_ESCAPE", Astra::Core::DiagnosticSeverity::Blocking, "DDC clean candidate escaped the cache root.", item.path());
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
            continue;
        }
        if (retained_paths.contains(candidate)) {
            ++report.retained;
            continue;
        }
        std::error_code error;
        if (std::filesystem::remove(candidate, error)) {
            ++report.removed;
        } else if (error) {
            auto diagnostic = MakeDiagnostic("ASTRA_DDC_CLEAN_REMOVE_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "DDC cache artifact could not be removed.", item.path());
            diagnostic.context["error"] = error.message();
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
    }
    return report;
}

AssetReleaseGateReport ValidateAssetReleaseGate(const AssetReleaseGateRequest& request, Astra::Core::DiagnosticSink& diagnostics) {
    AssetReleaseGateReport report;
    std::set<std::string> ids;
    for (const auto& entry : request.registry.entries) {
        ids.insert(entry.id.ToString());
    }
    std::set<std::string> seen_ids;
    for (const auto& entry : request.registry.entries) {
        const auto id = entry.id.ToString();
        if (!seen_ids.insert(id).second) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_ASSET_DUPLICATE_ID", Astra::Core::DiagnosticSeverity::Blocking, "Duplicate AssetId blocks release.", entry.sidecar_path);
            diagnostic.objects.push_back({"AssetId", id});
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
        if (entry.sidecar_path.empty() || !std::filesystem::exists(entry.sidecar_path)) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_ASSET_SIDECAR_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Asset sidecar is missing.", entry.source_path);
            diagnostic.objects.push_back({"AssetId", id});
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
        if (entry.source_path.empty() || !std::filesystem::exists(entry.source_path)) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_ASSET_SOURCE_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Asset source is missing.", entry.source_path);
            diagnostic.objects.push_back({"AssetId", id});
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
        if (entry.license.owner.empty() || entry.license.usage.empty()) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_ASSET_INVALID_LICENSE", Astra::Core::DiagnosticSeverity::Blocking, "Asset license owner and usage are required for release.", entry.sidecar_path);
            diagnostic.objects.push_back({"AssetId", id});
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
        if ((entry.requires_review || entry.origin == "AIGenerated" || entry.has_ai_generation) && entry.review.status != "accepted") {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_ASSET_004", Astra::Core::DiagnosticSeverity::Blocking, "Unreviewed AI or review-required asset cannot be packaged.", entry.sidecar_path);
            diagnostic.objects.push_back({"AssetId", id});
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
        if (entry.id.scheme == AssetScheme::Virtual) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_ASSET_VIRTUAL_UNRESOLVED", Astra::Core::DiagnosticSeverity::Blocking, "Virtual asset refs must be resolved before package release.", entry.sidecar_path);
            diagnostic.objects.push_back({"AssetId", id});
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
        if (entry.id.scheme == AssetScheme::Foreign && !request.allow_foreign_copy) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_ASSET_FOREIGN_COPY", Astra::Core::DiagnosticSeverity::Blocking, "Foreign assets are mount-only by default and cannot be copied into release packages.", entry.sidecar_path);
            diagnostic.objects.push_back({"AssetId", id});
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
        for (const auto& dependency : entry.hard_dependencies) {
            if (dependency.scheme == AssetScheme::Virtual) {
                auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_ASSET_VIRTUAL_UNRESOLVED", Astra::Core::DiagnosticSeverity::Blocking, "Virtual hard dependency must be resolved before package release.", entry.sidecar_path);
                diagnostic.objects.push_back({"AssetId", id});
                diagnostic.objects.push_back({"VirtualAssetId", dependency.ToString()});
                report.diagnostics.push_back(diagnostic);
                diagnostics.Emit(std::move(diagnostic));
            }
            if (!ids.contains(dependency.ToString())) {
                auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_ASSET_DEPENDENCY_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Hard dependency is missing from release graph.", entry.sidecar_path);
                diagnostic.objects.push_back({"AssetId", id});
                diagnostic.objects.push_back({"MissingAssetId", dependency.ToString()});
                report.diagnostics.push_back(diagnostic);
                diagnostics.Emit(std::move(diagnostic));
            }
        }
        for (const auto& dependency : entry.soft_dependencies) {
            if (dependency.scheme == AssetScheme::Virtual) {
                auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_ASSET_VIRTUAL_UNRESOLVED", Astra::Core::DiagnosticSeverity::Blocking, "Virtual soft dependency must be resolved before package release.", entry.sidecar_path);
                diagnostic.objects.push_back({"AssetId", id});
                diagnostic.objects.push_back({"VirtualAssetId", dependency.ToString()});
                report.diagnostics.push_back(diagnostic);
                diagnostics.Emit(std::move(diagnostic));
            }
            if (request.require_soft_dependencies && !ids.contains(dependency.ToString())) {
                auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_ASSET_SOFT_DEPENDENCY_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Release profile requires soft dependencies to resolve.", entry.sidecar_path);
                diagnostic.objects.push_back({"AssetId", id});
                diagnostic.objects.push_back({"MissingAssetId", dependency.ToString()});
                report.diagnostics.push_back(diagnostic);
                diagnostics.Emit(std::move(diagnostic));
            }
        }
        for (const auto& diagnostic : entry.diagnostics) {
            if (diagnostic.BlocksRelease()) {
                report.diagnostics.push_back(diagnostic);
                diagnostics.Emit(diagnostic);
            }
        }
    }

    std::map<std::string, DerivedDataCacheEntry> ddc_by_key;
    for (const auto& entry : request.cook_manifest.ddc_entries) {
        ddc_by_key[entry.key] = entry;
    }
    std::set<std::string> cooked_assets;
    for (const auto& artifact : request.cook_manifest.artifacts) {
        cooked_assets.insert(artifact.asset_id.ToString());
    }
    for (const auto& entry : request.registry.entries) {
        if (!cooked_assets.contains(entry.id.ToString())) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_COOK_ARTIFACT_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Registry asset has no cook artifact.", entry.sidecar_path);
            diagnostic.objects.push_back({"AssetId", entry.id.ToString()});
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
    }
    for (const auto& artifact : request.cook_manifest.artifacts) {
        if (artifact.ddc_key.empty() || artifact.hash.empty()) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_COOK_ARTIFACT_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Cook artifact is missing DDC key or hash.", artifact.artifact_path);
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
            continue;
        }
        if (!IsSupportedCookArtifact(artifact.processor_id, artifact.metadata) ||
            !artifact.metadata.value("package_eligible", true)) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_COOK_FORMAT_UNSUPPORTED", Astra::Core::DiagnosticSeverity::Blocking, "Cook artifact format or processor is not package eligible.", artifact.artifact_path);
            diagnostic.objects.push_back({"ProcessorId", artifact.processor_id});
            diagnostic.objects.push_back({"AssetId", artifact.asset_id.ToString()});
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
        const auto found = ddc_by_key.find(artifact.ddc_key);
        if (found == ddc_by_key.end() || found->second.output_hash != artifact.hash) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_DDC_HASH_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking, "Cook artifact hash does not match DDC metadata.", artifact.artifact_path);
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
    }
    for (const auto& module : request.package_manifest.modules) {
        if (!module.runtime_safe || module.sha256.empty()) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_MODULE_NOT_PACKAGE_ELIGIBLE", Astra::Core::DiagnosticSeverity::Blocking, "Package module is not runtime-safe or has no binary hash.", module.binary);
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
    }
    std::map<std::string, PackagePayloadEntry> payloads;
    for (const auto& payload : request.package_manifest.payloads) {
        payloads[payload.asset_id.ToString()] = payload;
    }
    for (const auto& artifact : request.package_manifest.cook_manifest.artifacts) {
        const auto found = payloads.find(artifact.asset_id.ToString());
        if (found == payloads.end()) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_PACKAGE_PAYLOAD_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Package payload is missing for cook artifact.", artifact.artifact_path);
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        } else if (found->second.hash != artifact.hash) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_PACKAGE_PAYLOAD_HASH_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking, "Package payload hash does not match cook artifact.", artifact.artifact_path);
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
    }
    report.passed = std::ranges::none_of(report.diagnostics, [](const auto& diagnostic) {
        return diagnostic.BlocksRelease();
    });
    report.evidence = {
        {"profile", request.profile},
        {"asset_count", request.registry.entries.size()},
        {"artifact_count", request.cook_manifest.artifacts.size()},
        {"package_hash", request.package_manifest.package_hash},
    };
    return report;
}

HotReloadTransaction PlanHotReloadTransaction(const AssetRegistry& before, const AssetRegistry& after, const AssetUri& asset_id, Astra::Core::DiagnosticSink& diagnostics) {
    AssetRegistryBuilder builder;
    const auto invalidations = builder.ComputeInvalidations(before, after);
    HotReloadTransaction transaction;
    transaction.transaction_id = "hotreload:/" + Sha256Text(asset_id.ToString() + "|" + ComputeAssetRegistryHash(after));
    transaction.asset_id = asset_id;
    const auto changed = std::ranges::find_if(invalidations, [&](const AssetInvalidation& invalidation) {
        return std::ranges::any_of(invalidation.affected_assets, [&](const AssetUri& affected) {
            return affected.ToString() == asset_id.ToString();
        });
    });
    if (changed == invalidations.end()) {
        transaction.stage = HotReloadStage::Detect;
        return transaction;
    }
    const auto after_entry = std::ranges::find_if(after.entries, [&](const AssetRegistryEntry& entry) {
        return entry.id.ToString() == asset_id.ToString();
    });
    if (after_entry == after.entries.end() || !after_entry->diagnostics.empty()) {
        transaction.stage = HotReloadStage::RolledBack;
        transaction.rollback_required = true;
        for (const auto& diagnostic : after_entry == after.entries.end() ? std::vector<Astra::Core::Diagnostic>{MakeDiagnostic("ASTRA_HOT_RELOAD_ASSET_REMOVED", Astra::Core::DiagnosticSeverity::Warning, "Hot reload asset was removed.")} : after_entry->diagnostics) {
            transaction.diagnostics.push_back(diagnostic);
            diagnostics.Emit(diagnostic);
        }
        return transaction;
    }
    transaction.stage = HotReloadStage::SwitchAtFrameBoundary;
    transaction.old_resource_retained = false;
    return transaction;
}

nlohmann::json ToJson(const AssetUri& uri) {
    return uri.ToString();
}

nlohmann::json ToJson(const AssetSidecar& sidecar) {
    nlohmann::json hard = nlohmann::json::array();
    for (const auto& dependency : sidecar.hard_dependencies) {
        hard.push_back(dependency.ToString());
    }
    nlohmann::json soft = nlohmann::json::array();
    for (const auto& dependency : sidecar.soft_dependencies) {
        soft.push_back(dependency.ToString());
    }
    return {
        {"schema", sidecar.schema},
        {"id", sidecar.id.ToString()},
        {"type", sidecar.type},
        {"source_path", sidecar.source_path.string()},
        {"display_name", sidecar.display_name},
        {"tags", sidecar.tags},
        {"origin", sidecar.origin},
        {"license", {{"owner", sidecar.license.owner}, {"usage", sidecar.license.usage}, {"source", sidecar.license.source}}},
        {"review", {{"status", sidecar.review.status}, {"review_item", sidecar.review.review_item}}},
        {"cook", {{"preset", sidecar.cook.preset}, {"settings", sidecar.cook.settings}}},
        {"dependencies", {{"hard", hard}, {"soft", soft}}},
        {"requires_review", sidecar.requires_review},
    };
}

nlohmann::json ToJson(const AssetRegistryEntry& entry) {
    nlohmann::json hard = nlohmann::json::array();
    for (const auto& dependency : entry.hard_dependencies) {
        hard.push_back(dependency.ToString());
    }
    nlohmann::json soft = nlohmann::json::array();
    for (const auto& dependency : entry.soft_dependencies) {
        soft.push_back(dependency.ToString());
    }
    nlohmann::json diagnostics = nlohmann::json::array();
    for (const auto& diagnostic : entry.diagnostics) {
        diagnostics.push_back(Astra::Core::ToJson(diagnostic));
    }
    return {
        {"id", entry.id.ToString()},
        {"type", entry.type},
        {"sidecar_path", entry.sidecar_path.string()},
        {"source_path", entry.source_path.string()},
        {"source_hash", entry.source_hash},
        {"sidecar_hash", entry.sidecar_hash},
        {"cook_key", entry.cook_key},
        {"importer_id", entry.importer_id},
        {"origin", entry.origin},
        {"license", {{"owner", entry.license.owner}, {"usage", entry.license.usage}, {"source", entry.license.source}}},
        {"review", {{"status", entry.review.status}, {"review_item", entry.review.review_item}}},
        {"requires_review", entry.requires_review},
        {"has_ai_generation", entry.has_ai_generation},
        {"dependencies", {{"hard", hard}, {"soft", soft}}},
        {"diagnostics", diagnostics},
    };
}

nlohmann::json ToJson(const AssetRegistry& registry) {
    nlohmann::json entries = nlohmann::json::array();
    for (const auto& entry : registry.entries) {
        entries.push_back(ToJson(entry));
    }
    return {{"schema", registry.schema}, {"version", registry.version}, {"entries", entries}};
}

AssetDependencyGraph BuildDependencyGraph(const AssetRegistry& registry) {
    AssetDependencyGraph graph;
    graph.assets = registry.entries;
    for (const auto& entry : registry.entries) {
        auto& hard = graph.hard_edges[entry.id.ToString()];
        for (const auto& dependency : entry.hard_dependencies) {
            hard.push_back(dependency.ToString());
        }
        auto& soft = graph.soft_edges[entry.id.ToString()];
        for (const auto& dependency : entry.soft_dependencies) {
            soft.push_back(dependency.ToString());
        }
    }
    return graph;
}

nlohmann::json ToJson(const AssetDependencyGraph& graph) {
    nlohmann::json assets = nlohmann::json::array();
    for (const auto& asset : graph.assets) {
        assets.push_back(ToJson(asset));
    }
    return {{"schema", graph.schema}, {"assets", assets}, {"hard_edges", graph.hard_edges}, {"soft_edges", graph.soft_edges}};
}

nlohmann::json ToJson(const ImportRequest& request) {
    return {
        {"schema", request.schema},
        {"source_path", request.source_path.string()},
        {"target_asset_id", request.target_asset_id.ToString()},
        {"asset_type", request.asset_type},
        {"preset", request.preset},
        {"origin", request.origin},
        {"review_state", request.review_state},
        {"license", {{"owner", request.license.owner}, {"usage", request.license.usage}, {"source", request.license.source}}},
        {"copy_source", request.copy_source},
        {"foreign_copy_allowed", request.foreign_copy_allowed},
    };
}

nlohmann::json ToJson(const ImporterDescriptor& descriptor) {
    return {
        {"provider_id", descriptor.provider_id},
        {"contract", descriptor.contract},
        {"source_extensions", descriptor.source_extensions},
        {"output_asset_types", descriptor.output_asset_types},
        {"sidecar_schema", descriptor.sidecar_schema},
        {"diagnostics_prefix", descriptor.diagnostics_prefix},
        {"capabilities", {{"preview_metadata", descriptor.preview_metadata}, {"batch_import", descriptor.batch_import}, {"ai_draft_import", descriptor.ai_draft_import}}},
        {"permissions", {{"project_write", descriptor.project_write}, {"foreign_read", descriptor.foreign_read}}},
    };
}

nlohmann::json ToJson(const DdcKey& key) {
    return {
        {"schema", key.schema},
        {"asset_id", key.asset_id.ToString()},
        {"source_hash", key.source_hash},
        {"sidecar_hash", key.sidecar_hash},
        {"processor_id", key.processor_id},
        {"processor_version", key.processor_version},
        {"platform", key.platform},
        {"profile", key.profile},
        {"provider_feature_hash", key.provider_feature_hash},
        {"key", key.ToString()},
    };
}

nlohmann::json ToJson(const CookArtifactDescriptor& descriptor) {
    nlohmann::json dependencies = nlohmann::json::array();
    for (const auto& dependency : descriptor.runtime_dependencies) {
        dependencies.push_back(dependency.ToString());
    }
    return {
        {"schema", descriptor.schema},
        {"artifact_id", descriptor.artifact_id},
        {"asset_id", descriptor.asset_id.ToString()},
        {"format", descriptor.format},
        {"ddc_key", descriptor.ddc_key},
        {"payload_hash", descriptor.payload_hash},
        {"payload_size", descriptor.payload_size},
        {"runtime_dependencies", dependencies},
        {"metadata", descriptor.metadata},
    };
}

nlohmann::json ToJson(const CookProcessorDescriptor& descriptor) {
    return {
        {"provider_id", descriptor.provider_id},
        {"contract", descriptor.contract},
        {"input_asset_types", descriptor.input_asset_types},
        {"output_formats", descriptor.output_formats},
        {"requires_providers", descriptor.requires_providers},
        {"package_eligible", descriptor.package_eligible},
        {"version", descriptor.version},
        {"diagnostics_prefix", descriptor.diagnostics_prefix},
    };
}

nlohmann::json ToJson(const DerivedDataCacheArtifact& artifact) {
    return {
        {"path", artifact.path.string()},
        {"hash", artifact.hash},
        {"size_bytes", artifact.size_bytes},
    };
}

nlohmann::json ToJson(const DerivedDataCacheEntry& entry) {
    nlohmann::json artifacts = nlohmann::json::array();
    for (const auto& artifact : entry.artifacts) {
        artifacts.push_back(ToJson(artifact));
    }
    return {
        {"schema", entry.schema},
        {"key", entry.key},
        {"processor_id", entry.processor_id},
        {"input_hash", entry.input_hash},
        {"settings_hash", entry.settings_hash},
        {"output_hash", entry.output_hash},
        {"platform", entry.platform},
        {"profile", entry.profile},
        {"engine_version", entry.engine_version},
        {"rebuilt", entry.rebuilt},
        {"corruption_recovered", entry.corruption_recovered},
        {"artifacts", artifacts},
    };
}

nlohmann::json ToJson(const DdcCleanReport& report) {
    nlohmann::json diagnostics = nlohmann::json::array();
    for (const auto& diagnostic : report.diagnostics) {
        diagnostics.push_back(Astra::Core::ToJson(diagnostic));
    }
    return {
        {"schema", report.schema},
        {"root", report.root.string()},
        {"scanned", report.scanned},
        {"retained", report.retained},
        {"removed", report.removed},
        {"diagnostics", diagnostics},
    };
}

nlohmann::json ToJson(const CookArtifact& artifact) {
    return {
        {"asset_id", artifact.asset_id.ToString()},
        {"type", artifact.type},
        {"artifact_path", artifact.artifact_path.string()},
        {"hash", artifact.hash},
        {"processor_id", artifact.processor_id},
        {"ddc_key", artifact.ddc_key},
        {"metadata", artifact.metadata},
    };
}

nlohmann::json ToJson(const CookManifest& manifest) {
    nlohmann::json artifacts = nlohmann::json::array();
    for (const auto& artifact : manifest.artifacts) {
        artifacts.push_back(ToJson(artifact));
    }
    nlohmann::json ddc_entries = nlohmann::json::array();
    for (const auto& entry : manifest.ddc_entries) {
        ddc_entries.push_back(ToJson(entry));
    }
    return {
        {"schema", manifest.schema},
        {"project_id", manifest.project_id},
        {"profile", manifest.profile},
        {"manifest_hash", manifest.manifest_hash},
        {"artifacts", artifacts},
        {"ddc_entries", ddc_entries},
    };
}

nlohmann::json ToJson(const PackageModuleEvidence& module) {
    return {
        {"id", module.id},
        {"binary", module.binary},
        {"sha256", module.sha256},
        {"runtime_safe", module.runtime_safe},
    };
}

nlohmann::json ToJson(const PackagePayloadEntry& payload) {
    return {
        {"asset_id", payload.asset_id.ToString()},
        {"artifact_path", payload.artifact_path.string()},
        {"hash", payload.hash},
        {"size_bytes", payload.size_bytes},
        {"encoding", payload.encoding},
        {"data", payload.data},
        {"offset", payload.offset},
        {"compressed_size", payload.compressed_size},
        {"compression", payload.compression},
        {"streaming", payload.streaming},
    };
}

nlohmann::json ToJson(const PackagePayloadRef& payload) {
    return {
        {"schema", payload.schema},
        {"asset_id", payload.asset_id.ToString()},
        {"artifact_id", payload.artifact_id},
        {"offset", payload.offset},
        {"size", payload.size},
        {"compressed_size", payload.compressed_size},
        {"hash", payload.hash},
        {"compression", payload.compression},
        {"streaming", payload.streaming},
    };
}

nlohmann::json ToJson(const PackageManifest& manifest) {
    nlohmann::json modules = nlohmann::json::array();
    for (const auto& module : manifest.modules) {
        modules.push_back(ToJson(module));
    }
    nlohmann::json payloads = nlohmann::json::array();
    for (const auto& payload : manifest.payloads) {
        payloads.push_back(ToJson(payload));
    }
    return {
        {"schema", manifest.schema},
        {"package_id", manifest.package_id},
        {"profile", manifest.profile},
        {"package_hash", manifest.package_hash},
        {"project_hash", manifest.project_hash},
        {"cook_manifest", ToJson(manifest.cook_manifest)},
        {"modules", modules},
        {"payloads", payloads},
        {"runtime_evidence", manifest.runtime_evidence},
    };
}

nlohmann::json ToJson(const PackageMountPolicy& policy) {
    return {
        {"schema", policy.schema},
        {"mount", policy.mount},
        {"read_only", policy.read_only},
        {"allow_random_access", policy.allow_random_access},
        {"allow_chunked_read", policy.allow_chunked_read},
        {"foreign_copy_allowed", policy.foreign_copy_allowed},
    };
}

nlohmann::json ToJson(const PackageMount& mount) {
    nlohmann::json assets = nlohmann::json::array();
    for (const auto& asset : mount.assets) {
        assets.push_back(asset.ToString());
    }
    return {
        {"schema", mount.schema},
        {"package_id", mount.package_id},
        {"package_path", mount.package_path.string()},
        {"read_only", mount.read_only},
        {"policy", ToJson(mount.policy)},
        {"assets", assets},
    };
}

nlohmann::json ToJson(const AssetReleaseGateReport& report) {
    nlohmann::json diagnostics = nlohmann::json::array();
    for (const auto& diagnostic : report.diagnostics) {
        diagnostics.push_back(Astra::Core::ToJson(diagnostic));
    }
    return {
        {"schema", report.schema},
        {"passed", report.passed},
        {"diagnostics", diagnostics},
        {"evidence", report.evidence},
    };
}

std::string ToString(HotReloadStage stage) {
    switch (stage) {
    case HotReloadStage::Detect:
        return "detect";
    case HotReloadStage::Validate:
        return "validate";
    case HotReloadStage::CookTemp:
        return "cook_temp";
    case HotReloadStage::PrepareProviderResource:
        return "prepare_provider_resource";
    case HotReloadStage::SwitchAtFrameBoundary:
        return "switch_at_frame_boundary";
    case HotReloadStage::RetireOldResource:
        return "retire_old_resource";
    case HotReloadStage::RolledBack:
        return "rolled_back";
    }
    return "detect";
}

nlohmann::json ToJson(const HotReloadTransaction& transaction) {
    nlohmann::json diagnostics = nlohmann::json::array();
    for (const auto& diagnostic : transaction.diagnostics) {
        diagnostics.push_back(Astra::Core::ToJson(diagnostic));
    }
    return {
        {"schema", transaction.schema},
        {"transaction_id", transaction.transaction_id},
        {"asset_id", transaction.asset_id.ToString()},
        {"stage", ToString(transaction.stage)},
        {"rollback_required", transaction.rollback_required},
        {"old_resource_retained", transaction.old_resource_retained},
        {"provider_id", transaction.provider_id},
        {"diagnostics", diagnostics},
    };
}

Astra::Core::Result<PackageManifest> PackageManifestFromJson(const nlohmann::json& json) {
    if (json.value("schema", std::string()) != PackageManifestSchema) {
        return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "unsupported package manifest schema");
    }
    PackageManifest manifest;
    manifest.package_id = json.value("package_id", "");
    manifest.profile = json.value("profile", "deterministic");
    manifest.package_hash = json.value("package_hash", "");
    manifest.project_hash = json.value("project_hash", "");
    if (json.contains("cook_manifest")) {
        const auto& cook = json.at("cook_manifest");
        manifest.cook_manifest.project_id = cook.value("project_id", "");
        manifest.cook_manifest.profile = cook.value("profile", "");
        manifest.cook_manifest.manifest_hash = cook.value("manifest_hash", "");
        for (const auto& artifact_json : cook.value("artifacts", nlohmann::json::array())) {
            auto asset_id = ParseAssetUri(artifact_json.value("asset_id", "asset:/invalid"));
            if (!asset_id) {
                continue;
            }
            CookArtifact artifact;
            artifact.asset_id = asset_id.Value();
            artifact.type = artifact_json.value("type", "");
            artifact.artifact_path = artifact_json.value("artifact_path", "");
            artifact.hash = artifact_json.value("hash", "");
            artifact.processor_id = artifact_json.value("processor_id", "");
            artifact.ddc_key = artifact_json.value("ddc_key", "");
            artifact.metadata = artifact_json.value("metadata", nlohmann::json::object());
            manifest.cook_manifest.artifacts.push_back(std::move(artifact));
        }
        for (const auto& ddc_json : cook.value("ddc_entries", nlohmann::json::array())) {
            DerivedDataCacheEntry entry;
            entry.schema = ddc_json.value("schema", DerivedDataCacheEntrySchema);
            entry.key = ddc_json.value("key", "");
            entry.processor_id = ddc_json.value("processor_id", "");
            entry.input_hash = ddc_json.value("input_hash", "");
            entry.settings_hash = ddc_json.value("settings_hash", "");
            entry.output_hash = ddc_json.value("output_hash", "");
            entry.platform = ddc_json.value("platform", "win64");
            entry.profile = ddc_json.value("profile", "");
            entry.engine_version = ddc_json.value("engine_version", "");
            entry.rebuilt = ddc_json.value("rebuilt", false);
            entry.corruption_recovered = ddc_json.value("corruption_recovered", false);
            for (const auto& artifact_json : ddc_json.value("artifacts", nlohmann::json::array())) {
                entry.artifacts.push_back({
                    artifact_json.value("path", ""),
                    artifact_json.value("hash", ""),
                    artifact_json.value("size_bytes", static_cast<Astra::Core::u64>(0)),
                });
            }
            manifest.cook_manifest.ddc_entries.push_back(std::move(entry));
        }
    }
    for (const auto& module_json : json.value("modules", nlohmann::json::array())) {
        manifest.modules.push_back({
            module_json.value("id", ""),
            module_json.value("binary", ""),
            module_json.value("sha256", ""),
            module_json.value("runtime_safe", true),
        });
    }
    for (const auto& payload_json : json.value("payloads", nlohmann::json::array())) {
        auto asset_id = ParseAssetUri(payload_json.value("asset_id", "asset:/invalid"));
        if (!asset_id) {
            continue;
        }
        PackagePayloadEntry payload;
        payload.asset_id = asset_id.Value();
        payload.artifact_path = payload_json.value("artifact_path", "");
        payload.hash = payload_json.value("hash", "");
        payload.size_bytes = payload_json.value("size_bytes", static_cast<Astra::Core::u64>(0));
        payload.encoding = payload_json.value("encoding", "base64");
        payload.data = payload_json.value("data", "");
        payload.offset = payload_json.value("offset", static_cast<Astra::Core::u64>(0));
        payload.compressed_size = payload_json.value("compressed_size", static_cast<Astra::Core::u64>(0));
        payload.compression = payload_json.value("compression", payload.encoding == "base64" ? "none" : "zstd");
        payload.streaming = payload_json.value("streaming", payload.offset > 0 ? "chunked" : "inline");
        manifest.payloads.push_back(std::move(payload));
    }
    manifest.runtime_evidence = json.value("runtime_evidence", nlohmann::json::object());
    return Astra::Core::Result<PackageManifest>::Success(std::move(manifest));
}

std::string ComputeCookManifestHash(nlohmann::json manifest_json) {
    manifest_json["manifest_hash"] = "";
    return Sha256Text(manifest_json.dump());
}

std::string ComputePackageManifestHash(nlohmann::json manifest_json) {
    manifest_json["package_hash"] = "";
    return Sha256Text(manifest_json.dump());
}

Astra::Core::Result<PackageManifest> PackageReader::ReadManifest(const std::filesystem::path& package_path, Astra::Core::DiagnosticSink& diagnostics) const {
    auto package_json = ReadPackageJson(package_path, diagnostics);
    if (!package_json) {
        return Astra::Core::Result<PackageManifest>::Failure(package_json.Error(), package_json.Message());
    }
    auto json = package_json.Value();
    auto manifest = PackageManifestFromJson(json);
    if (!manifest) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_MANIFEST_SCHEMA", Astra::Core::DiagnosticSeverity::Blocking, manifest.Message(), package_path));
        return manifest;
    }

    const auto expected_package_hash = json.value("package_hash", "");
    const auto actual_package_hash = ComputePackageManifestHash(json);
    if (expected_package_hash.empty() || expected_package_hash != actual_package_hash) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_HASH_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking, "Package manifest hash does not match package contents.", package_path));
        return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package hash mismatch");
    }

    if (json.contains("cook_manifest")) {
        const auto& cook_json = json.at("cook_manifest");
        const auto expected_cook_hash = cook_json.value("manifest_hash", "");
        const auto actual_cook_hash = ComputeCookManifestHash(cook_json);
        if (expected_cook_hash.empty() || expected_cook_hash != actual_cook_hash) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_COOK_MANIFEST_HASH_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking, "Cook manifest hash does not match cook artifacts and DDC metadata.", package_path));
            return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "cook manifest hash mismatch");
        }
    }

    std::map<std::string, DerivedDataCacheEntry> ddc_by_key;
    for (const auto& entry : manifest.Value().cook_manifest.ddc_entries) {
        if (entry.key.empty() || entry.output_hash.empty() || entry.processor_id.empty()) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_DDC_ENTRY_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "DDC entry is missing key, processor, or output hash.", package_path));
            return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "ddc entry invalid");
        }
        ddc_by_key[entry.key] = entry;
    }
    for (const auto& artifact : manifest.Value().cook_manifest.artifacts) {
        if (artifact.ddc_key.empty()) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_DDC_ARTIFACT_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Cook artifact is missing its DDC key.", package_path));
            return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "artifact ddc key missing");
        }
        const auto found = ddc_by_key.find(artifact.ddc_key);
        if (found == ddc_by_key.end() || found->second.output_hash != artifact.hash) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_DDC_ARTIFACT_HASH_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking, "Cook artifact hash does not match DDC metadata.", package_path));
            return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "artifact ddc hash mismatch");
        }
    }

    std::map<std::string, CookArtifact> artifacts_by_asset;
    for (const auto& artifact : manifest.Value().cook_manifest.artifacts) {
        artifacts_by_asset[artifact.asset_id.ToString()] = artifact;
    }
    std::set<std::string> payload_assets;
    for (const auto& payload : manifest.Value().payloads) {
        const auto asset = payload.asset_id.ToString();
        if (payload.encoding == "base64") {
            auto decoded = DecodeBase64(payload.data);
            if (!decoded) {
                diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_ENCODING", Astra::Core::DiagnosticSeverity::Blocking, decoded.Message(), package_path));
                return Astra::Core::Result<PackageManifest>::Failure(decoded.Error(), decoded.Message());
            }
            if (decoded.Value().size() != payload.size_bytes) {
                diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_SIZE_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking, "Package payload decoded size does not match manifest metadata.", package_path));
                return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package payload size mismatch");
            }
            const auto payload_hash = Sha256Bytes(decoded.Value());
            if (payload.hash.empty() || payload_hash != payload.hash) {
                diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_HASH_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking, "Package payload hash does not match decoded bytes.", package_path));
                return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package payload hash mismatch");
            }
        } else if (payload.encoding == "binary") {
            if (payload.offset < BinaryPackageHeaderSize() || payload.compressed_size == 0 || payload.size_bytes == 0) {
                diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_REF_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Binary package payload ref is invalid.", package_path));
                return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package payload ref invalid");
            }
            std::ifstream package_file(package_path, std::ios::binary);
            package_file.seekg(static_cast<std::streamoff>(payload.offset));
            std::vector<Astra::Core::u8> compressed(static_cast<std::size_t>(payload.compressed_size));
            package_file.read(reinterpret_cast<char*>(compressed.data()), static_cast<std::streamsize>(compressed.size()));
            if (!package_file) {
                diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_TRUNCATED", Astra::Core::DiagnosticSeverity::Blocking, "Binary package payload is truncated.", package_path));
                return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package payload truncated");
            }
            Astra::Core::Result<std::vector<Astra::Core::u8>> decoded =
                payload.compression == "zstd"
                    ? DecompressZstd(compressed, payload.size_bytes)
                    : Astra::Core::Result<std::vector<Astra::Core::u8>>::Success(std::move(compressed));
            if (!decoded) {
                diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_COMPRESSION", Astra::Core::DiagnosticSeverity::Blocking, decoded.Message(), package_path));
                return Astra::Core::Result<PackageManifest>::Failure(decoded.Error(), decoded.Message());
            }
            const auto payload_hash = Sha256Bytes(decoded.Value());
            if (payload.hash.empty() || payload_hash != payload.hash) {
                diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_HASH_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking, "Package payload hash does not match binary payload bytes.", package_path));
                return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package payload hash mismatch");
            }
        } else {
            diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_ENCODING", Astra::Core::DiagnosticSeverity::Blocking, "Package payload uses an unsupported encoding.", package_path));
            return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package payload encoding unsupported");
        }
        const auto artifact = artifacts_by_asset.find(asset);
        if (artifact != artifacts_by_asset.end() && artifact->second.hash != payload.hash) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_HASH_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking, "Package payload hash does not match the cook artifact hash.", package_path));
            return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package payload artifact hash mismatch");
        }
        payload_assets.insert(asset);
    }
    for (const auto& artifact : manifest.Value().cook_manifest.artifacts) {
        if (!payload_assets.contains(artifact.asset_id.ToString())) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Cook artifact has no embedded package payload.", package_path));
            return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package payload missing");
        }
    }
    return manifest;
}

Astra::Core::Result<std::vector<Astra::Core::u8>> PackageReader::ReadPayloadBytes(const std::filesystem::path& package_path, const AssetUri& asset_id, Astra::Core::DiagnosticSink& diagnostics) const {
    auto manifest = ReadManifest(package_path, diagnostics);
    if (!manifest) {
        return Astra::Core::Result<std::vector<Astra::Core::u8>>::Failure(manifest.Error(), manifest.Message());
    }
    const auto requested = asset_id.ToString();
    const auto found = std::ranges::find_if(manifest.Value().payloads, [&](const PackagePayloadEntry& payload) {
        return payload.asset_id.ToString() == requested;
    });
    if (found == manifest.Value().payloads.end()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_NOT_FOUND", Astra::Core::DiagnosticSeverity::Blocking, "Requested asset payload is not present in the package.", package_path));
        return Astra::Core::Result<std::vector<Astra::Core::u8>>::Failure(Astra::Core::ErrorCode::NotFound, "package payload not found");
    }
    if (found->encoding == "base64") {
        auto decoded = DecodeBase64(found->data);
        if (!decoded) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_ENCODING", Astra::Core::DiagnosticSeverity::Blocking, decoded.Message(), package_path));
            return decoded;
        }
        return decoded;
    }
    if (found->encoding != "binary") {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_ENCODING", Astra::Core::DiagnosticSeverity::Blocking, "Package payload uses an unsupported encoding.", package_path));
        return Astra::Core::Result<std::vector<Astra::Core::u8>>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package payload encoding unsupported");
    }
    std::ifstream file(package_path, std::ios::binary);
    file.seekg(static_cast<std::streamoff>(found->offset));
    std::vector<Astra::Core::u8> compressed(static_cast<std::size_t>(found->compressed_size));
    file.read(reinterpret_cast<char*>(compressed.data()), static_cast<std::streamsize>(compressed.size()));
    if (!file) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_TRUNCATED", Astra::Core::DiagnosticSeverity::Blocking, "Binary package payload is truncated.", package_path));
        return Astra::Core::Result<std::vector<Astra::Core::u8>>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package payload truncated");
    }
    auto decoded = found->compression == "zstd"
                       ? DecompressZstd(compressed, found->size_bytes)
                       : Astra::Core::Result<std::vector<Astra::Core::u8>>::Success(std::move(compressed));
    if (!decoded) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_COMPRESSION", Astra::Core::DiagnosticSeverity::Blocking, decoded.Message(), package_path));
        return decoded;
    }
    if (Sha256Bytes(decoded.Value()) != found->hash) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_HASH_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking, "Package payload hash does not match binary payload bytes.", package_path));
        return Astra::Core::Result<std::vector<Astra::Core::u8>>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package payload hash mismatch");
    }
    return decoded;
}

Astra::Core::Result<std::vector<PackagePayloadChunk>> PackageReader::ReadPayloadChunks(const std::filesystem::path& package_path, const AssetUri& asset_id, Astra::Core::u64 chunk_size, Astra::Core::DiagnosticSink& diagnostics) const {
    if (chunk_size == 0) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_CHUNK_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Package payload chunk size must be greater than zero.", package_path));
        return Astra::Core::Result<std::vector<PackagePayloadChunk>>::Failure(Astra::Core::ErrorCode::InvalidArgument, "package payload chunk size invalid");
    }
    auto bytes = ReadPayloadBytes(package_path, asset_id, diagnostics);
    if (!bytes) {
        return Astra::Core::Result<std::vector<PackagePayloadChunk>>::Failure(bytes.Error(), bytes.Message());
    }
    std::vector<PackagePayloadChunk> chunks;
    for (Astra::Core::u64 offset = 0; offset < bytes.Value().size(); offset += chunk_size) {
        const auto remaining = static_cast<Astra::Core::u64>(bytes.Value().size()) - offset;
        const auto count = static_cast<std::size_t>(std::min(chunk_size, remaining));
        const auto begin = bytes.Value().begin() + static_cast<std::ptrdiff_t>(offset);
        PackagePayloadChunk chunk;
        chunk.asset_id = asset_id;
        chunk.offset = offset;
        chunk.bytes.assign(begin, begin + static_cast<std::ptrdiff_t>(count));
        chunk.final_chunk = offset + count == bytes.Value().size();
        chunks.push_back(std::move(chunk));
    }
    if (bytes.Value().empty()) {
        chunks.push_back({asset_id, 0, {}, true});
    }
    return Astra::Core::Result<std::vector<PackagePayloadChunk>>::Success(std::move(chunks));
}

Astra::Core::Result<std::string> PackageReader::ReadPayloadText(const std::filesystem::path& package_path, const AssetUri& asset_id, Astra::Core::DiagnosticSink& diagnostics) const {
    auto bytes = ReadPayloadBytes(package_path, asset_id, diagnostics);
    if (!bytes) {
        return Astra::Core::Result<std::string>::Failure(bytes.Error(), bytes.Message());
    }
    return Astra::Core::Result<std::string>::Success(std::string(bytes.Value().begin(), bytes.Value().end()));
}

Astra::Core::Result<PackageMount> PackageReader::MountPackage(const std::filesystem::path& package_path, Astra::Core::DiagnosticSink& diagnostics) const {
    auto manifest = ReadManifest(package_path, diagnostics);
    if (!manifest) {
        return Astra::Core::Result<PackageMount>::Failure(manifest.Error(), manifest.Message());
    }
    PackageMount mount;
    mount.package_id = manifest.Value().package_id;
    mount.package_path = std::filesystem::absolute(package_path).lexically_normal();
    for (const auto& payload : manifest.Value().payloads) {
        mount.assets.push_back(payload.asset_id);
    }
    return Astra::Core::Result<PackageMount>::Success(std::move(mount));
}

Astra::Core::Result<PackageManifest> PackageWriter::WritePackage(PackageManifest manifest, const std::filesystem::path& package_path, Astra::Core::DiagnosticSink& diagnostics) const {
    std::vector<std::vector<Astra::Core::u8>> compressed_payloads;
    compressed_payloads.reserve(manifest.cook_manifest.artifacts.size());
    manifest.payloads.clear();

    for (const auto& artifact : manifest.cook_manifest.artifacts) {
        if (!std::filesystem::exists(artifact.artifact_path)) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_SOURCE_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Cook artifact payload is missing.", artifact.artifact_path));
            return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::NotFound, "package payload source missing");
        }
        const auto bytes = ReadBytes(artifact.artifact_path);
        auto compressed = CompressZstd(bytes);
        if (compressed.empty() && !bytes.empty()) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_COMPRESSION", Astra::Core::DiagnosticSeverity::Blocking, "zstd compression failed for package payload.", artifact.artifact_path));
            return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InternalError, "package payload compression failed");
        }
        PackagePayloadEntry payload;
        payload.asset_id = artifact.asset_id;
        payload.artifact_path = artifact.artifact_path;
        payload.hash = Sha256Bytes(bytes);
        payload.size_bytes = static_cast<Astra::Core::u64>(bytes.size());
        payload.encoding = "binary";
        payload.compressed_size = static_cast<Astra::Core::u64>(compressed.size());
        payload.compression = "zstd";
        payload.streaming = "chunked";
        manifest.payloads.push_back(std::move(payload));
        compressed_payloads.push_back(std::move(compressed));
    }

    if (manifest.profile.empty()) {
        manifest.profile = "deterministic";
    }
    if (manifest.cook_manifest.manifest_hash.empty()) {
        manifest.cook_manifest.manifest_hash = ComputeCookManifestHash(ToJson(manifest.cook_manifest));
    }
    manifest.package_hash = std::string(64, '0');

    nlohmann::json package_json;
    for (int pass = 0; pass < 3; ++pass) {
        package_json = ToJson(manifest);
        const auto manifest_text = package_json.dump();
        auto offset = BinaryPackageHeaderSize() + static_cast<Astra::Core::u64>(manifest_text.size());
        for (std::size_t index = 0; index < manifest.payloads.size(); ++index) {
            manifest.payloads[index].offset = offset;
            manifest.payloads[index].compressed_size = static_cast<Astra::Core::u64>(compressed_payloads[index].size());
            offset += manifest.payloads[index].compressed_size;
        }
        package_json = ToJson(manifest);
        package_json["package_hash"] = ComputePackageManifestHash(package_json);
        manifest.package_hash = package_json["package_hash"].get<std::string>();
    }

    const auto final_json = ToJson(manifest);
    const auto manifest_text = final_json.dump();
    std::filesystem::create_directories(package_path.parent_path());
    std::ofstream file(package_path, std::ios::binary);
    if (!file) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_WRITE_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "Package file could not be opened for writing.", package_path));
        return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::PermissionDenied, "package write failed");
    }
    file.write(kPackageMagic.data(), static_cast<std::streamsize>(kPackageMagic.size()));
    WritePod(file, kPackageVersion);
    WritePod(file, static_cast<Astra::Core::u64>(manifest_text.size()));
    file.write(manifest_text.data(), static_cast<std::streamsize>(manifest_text.size()));
    for (const auto& payload : compressed_payloads) {
        if (!payload.empty()) {
            file.write(reinterpret_cast<const char*>(payload.data()), static_cast<std::streamsize>(payload.size()));
        }
    }
    if (!file) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_WRITE_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "Package file write failed.", package_path));
        return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InternalError, "package write failed");
    }
    return Astra::Core::Result<PackageManifest>::Success(std::move(manifest));
}

std::string ComputeAssetRegistryHash(const AssetRegistry& registry) {
    return Sha256Text(ToJson(registry).dump());
}

std::string ComputeProviderFeatureHash(const std::map<std::string, std::string>& selected_providers) {
    nlohmann::json json = nlohmann::json::object();
    for (const auto& [slot, provider] : selected_providers) {
        json[slot] = provider;
    }
    if (json.empty()) {
        json["astra.asset"] = "builtin.phase6";
    }
    return Sha256Text(json.dump());
}

} // namespace Astra::Asset
