/* The official 0.17.4 headers own va_list and ASS_Track ABI details. */
#include <ass/ass.h>
#include <string.h>

#if LIBASS_VERSION != 0x01704000
#error astra-subtitle requires libass 0.17.4 headers
#endif

/* Sticky status: 1 native diagnostic, 2 missing font, 3 missing glyph.
 * Never format arguments: they can contain private text, paths and font names.
 * A C callback is necessary because va_list is platform-specific. */
static void quiet_message(int level, const char *fmt, va_list args, void *data)
{
    int *status = (int *)data;
    int failure = 0;
    if (strstr(fmt, "Glyph 0x") && strstr(fmt, "not found"))
        failure = 3;
    else if (strstr(fmt, "fontselect: failed to find any fallback")) {
        va_list copy;
        unsigned code;
        va_copy(copy, args);
        code = va_arg(copy, unsigned);
        va_end(copy);
        failure = code ? 3 : 2;
    } else if (strstr(fmt, "fontselect: Using default"))
        failure = 2;
    else if (level <= 2)
        failure = 1;
    if (failure && !*status)
        *status = failure;
}

void astra_ass_quiet(ASS_Library *library, int *status)
{
    ass_set_message_cb(library, quiet_message, status);
    /* Only fonts passed explicitly through ass_add_font are available. */
    ass_set_extract_fonts(library, 0);
    ass_set_fonts_dir(library, NULL);
}

int astra_ass_matrix(const ASS_Track *track)
{
    return track->YCbCrMatrix;
}

int astra_ass_track_valid(const ASS_Track *track)
{
    return track->track_type == TRACK_TYPE_ASS && track->event_format &&
           track->PlayResX > 0 && track->PlayResY > 0;
}
