#include <Astra/Asset/Asset.hpp>

#include "AssetInternal.hpp"

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

#include <csetjmp>
#include <cstring>

namespace Astra::Asset {
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


} // namespace Astra::Asset
