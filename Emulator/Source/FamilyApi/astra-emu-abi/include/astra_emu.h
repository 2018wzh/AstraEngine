#ifndef ASTRA_EMU_ABI_H
#define ASTRA_EMU_ABI_H

/* Generated from astra-emu-abi Rust types by cbindgen 0.29.2. Do not edit. */

#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <stdint.h>
#include <stddef.h>

#define AstraEmuABI_VERSION 3

#define AstraEmuMAX_CONFIG_FIELDS 128

#define AstraEmuMAX_CONFIG_TEXT 4096

#define AstraEmuVALUE_BOOL 1

#define AstraEmuVALUE_INTEGER 2

#define AstraEmuVALUE_NUMBER 3

#define AstraEmuVALUE_STRING 4

#define AstraEmuVALUE_ENUM 5

#define AstraEmuMAX_CONFIG_DESCRIPTION 512

#define AstraEmuMAX_CHOICE_LABEL 64

#define AstraEmuMAX_CONFIG_UNIT 16

#define AstraEmuCONFIG_FLAG_FILE_PATH 1

#define AstraEmuCONFIG_FLAG_DIRECTORY_PATH 2

#define AstraEmuCONFIG_FLAG_ADVANCED 4

#define AstraEmuCONFIG_FLAG_GAME_RELATIVE 8

#define AstraEmuCAP_CPU_SURFACE 1

#define AstraEmuCAP_TEXT_HOOK 4

#define AstraEmuCAP_LIVE_CONFIGURATION 8

#define AstraEmuCAP_RENDER_EXTENT 16

#define AstraEmuMAX_DISPLAY_NAME_BYTES 64

#define AstraEmuMAX_RENDER_EXTENT 16384

#define AstraEmuRENDER_PRESERVE_ASPECT 1

#define AstraEmuINPUT_KEY 1

#define AstraEmuINPUT_POINTER_MOVE 2

#define AstraEmuINPUT_POINTER_BUTTON 3

#define AstraEmuINPUT_WHEEL 4

#define AstraEmuINPUT_WINDOW 5

#define AstraEmuBUTTON_PRIMARY 1

#define AstraEmuBUTTON_SECONDARY 2

#define AstraEmuBUTTON_MIDDLE 3

#define AstraEmuCONTROL_PAUSE 1

#define AstraEmuCONTROL_RESUME 2

#define AstraEmuCONTROL_RETRY 3

#define AstraEmuSESSION_RUNNING 1

#define AstraEmuSESSION_PAUSED 2

#define AstraEmuSESSION_WAITING 3

#define AstraEmuSESSION_FAILED 4

#define AstraEmuSESSION_FINISHED 5

/**
 * Only a retained, retry-safe operation may set this flag in SESSION_FAILED.
 * Panic and fatal native failures leave it clear; Host offers End only.
 */
#define AstraEmuSESSION_CAN_RETRY 1

#define AstraEmuMODULE_OWNER_VERSION 1

#define AstraEmuLOG_ERROR 1

#define AstraEmuLOG_WARN 2

#define AstraEmuLOG_INFO 3

#define AstraEmuLOG_DEBUG 4

#define AstraEmuLOG_TRACE 5

#define AstraEmuFIELD_TEXT 1

#define AstraEmuFIELD_I64 2

#define AstraEmuFIELD_U64 3

#define AstraEmuFIELD_F64 4

#define AstraEmuFIELD_BOOL 5

#define AstraEmuTEXT_DIALOGUE 1

#define AstraEmuTEXT_SPEAKER 2

#define AstraEmuTEXT_CHOICE 3

#define AstraEmuWINDOW_TITLE 1

#define AstraEmuWINDOW_FULLSCREEN 2

#define AstraEmuWINDOW_ICON 3

#define AstraEmuWINDOW_POINTER 4

#define AstraEmuWINDOW_MODAL 5

#define AstraEmuWINDOW_SIZE 6

#define AstraEmuWINDOW_POSITION 7

/**
 * flags is exactly 0/1; Host reports actual state using matching result bit.
 */
#define AstraEmuWINDOW_VISIBILITY 8

#define AstraEmuWINDOW_MINIMIZED 9

/**
 * Read actual window state without mutation. flags must be zero; inspect result valid_fields.
 */
#define AstraEmuWINDOW_QUERY_STATE 10

#define AstraEmuMODAL_TEXT_INPUT 1

#define AstraEmuMAX_MODAL_TEXT 65536

#define AstraEmuMAX_MODAL_BUTTONS 16

#define AstraEmuMAX_MODAL_BUTTON_LABEL 256

#define AstraEmuWINDOW_RESULT_MODAL 1

#define AstraEmuWINDOW_RESULT_SIZE 2

#define AstraEmuWINDOW_RESULT_POSITION 4

#define AstraEmuWINDOW_RESULT_MODE 8

#define AstraEmuWINDOW_RESULT_VISIBILITY 16

#define AstraEmuWINDOW_RESULT_MINIMIZED 32

#define AstraEmuFULLSCREEN_WINDOWED 0

#define AstraEmuFULLSCREEN_BORDERLESS 1

#define AstraEmuFULLSCREEN_EXCLUSIVE 2

#define AstraEmuPOINTER_SET_POSITION 1

#define AstraEmuPOINTER_SET_VISIBILITY 2

#define AstraEmuPOINTER_VISIBLE 4

#define AstraEmuPOINTER_SET_AUTOHIDE 8

#define AstraEmuPOINTER_SET_X 16

#define AstraEmuPOINTER_SET_Y 32

#define AstraEmuSESSION_MENU_VERSION 1

/**
 * Core-defined control commands start here; built-in CONTROL_* stay below.
 */
#define AstraEmuCONTROL_CORE_BASE 4096

#define AstraEmuSESSION_MENU_MAX_ITEMS 16

#define AstraEmuSESSION_MENU_LABEL_MAX 64

#define AstraEmuMENU_ICON_GENERIC 0

#define AstraEmuMENU_ICON_QUICK_SAVE 1

#define AstraEmuMENU_ICON_QUICK_LOAD 2

#define AstraEmuMENU_ICON_SAVE 3

#define AstraEmuMENU_ICON_LOAD 4

#define AstraEmuMENU_ICON_AUTO 5

#define AstraEmuMENU_ICON_SKIP 6

#define AstraEmuMENU_ICON_BACKLOG 7

#define AstraEmuMENU_ICON_SETTINGS 8

#define AstraEmuSURFACE_CPU 1

#define AstraEmuPIXEL_RGBA8 1

#define AstraEmuPIXEL_BGRA8 2

#define AstraEmuCOLOR_SRGB 1

#define AstraEmuCOLOR_LINEAR_SRGB 2

#define AstraEmuALPHA_OPAQUE 1

#define AstraEmuALPHA_IGNORED 2

#define AstraEmuALPHA_STRAIGHT 3

#define AstraEmuALPHA_PREMULTIPLIED 4

#define AstraEmuROWS_TOP_DOWN 1

#define AstraEmuROWS_BOTTOM_UP 2

#define AstraEmuFRAME_PENDING 0

#define AstraEmuFRAME_READY 1

#define AstraEmuFRAME_UNCHANGED 2

#define AstraEmuMAX_FRAME_BYTES ((512 * 1024) * 1024)

typedef int32_t AstraEmuStatus;

typedef struct AstraEmuHeader {
  uint32_t size;
  uint32_t version;
} AstraEmuHeader;

/**
 * Borrowed bytes. Null is permitted only for an empty span. The owner keeps
 * storage immutable and alive for the whole call (descriptors: library life).
 */
typedef struct AstraEmuBytes {
  const uint8_t *data;
  size_t len;
} AstraEmuBytes;

typedef struct AstraEmuLogField {
  struct AstraEmuBytes name;
  uint32_t kind;
  uint32_t reserved;
  struct AstraEmuBytes text;
  int64_t signed_;
  uint64_t unsigned_;
  double number;
} AstraEmuLogField;

typedef struct AstraEmuLogRecord {
  uint32_t level;
  uint32_t dropped_fields;
  /**
   * Empty targets identify unstructured native logs; the sink names them.
   */
  struct AstraEmuBytes target;
  /**
   * May be empty for native logging systems without structured event names.
   */
  struct AstraEmuBytes event;
  struct AstraEmuBytes message;
  const struct AstraEmuLogField *fields;
  size_t field_count;
} AstraEmuLogRecord;

/**
 * Installed before descriptor/probe/open; sink storage lives until process exit.
 * Callback is thread-safe; all spans are borrowed only for this invocation.
 */
typedef struct AstraEmuDiagnostics {
  struct AstraEmuHeader header;
  void *context;
  void (*emit)(void*, const struct AstraEmuLogRecord*);
  void (*flush)(void*);
} AstraEmuDiagnostics;

/**
 * Caller-owned error output. Every failure supplies a stable diagnostic code;
 * status remains machine-readable even when the optional error pointer is null.
 */
typedef struct AstraEmuError {
  uint32_t code_len;
  uint32_t message_len;
  uint8_t code[128];
  uint8_t message[512];
} AstraEmuError;

/**
 * Only the member selected by `kind` is meaningful. Boolean uses integer 0/1.
 * Reserved members must be zero; no Rust enum or owning string crosses the ABI.
 */
typedef struct AstraEmuConfigValue {
  uint32_t kind;
  uint32_t reserved;
  int64_t integer;
  double number;
  struct AstraEmuBytes text;
} AstraEmuConfigValue;

/**
 * Path flags apply only to STRING fields and are mutually exclusive;
 * GAME_RELATIVE requires one of them. `choice_labels` is null when
 * `choice_count` is zero, otherwise exactly `choice_count` user-visible labels
 * parallel to `choices`. `step` is 0 for none, a positive finite value for
 * NUMBER, or an integral value of at least 1 for INTEGER.
 */
typedef struct AstraEmuConfigField {
  struct AstraEmuBytes id;
  struct AstraEmuBytes label;
  struct AstraEmuBytes group;
  struct AstraEmuConfigValue default_value;
  int64_t integer_min;
  int64_t integer_max;
  double number_min;
  double number_max;
  uint32_t max_bytes;
  uint32_t flags;
  const struct AstraEmuBytes *choices;
  size_t choice_count;
  struct AstraEmuBytes description;
  const struct AstraEmuBytes *choice_labels;
  double step;
  struct AstraEmuBytes unit;
} AstraEmuConfigField;

typedef struct AstraEmuExtent {
  uint32_t width;
  uint32_t height;
} AstraEmuExtent;

/**
 * Render extents a core accepts in `OpenRequest.render_extent`. All members
 * are zero unless the descriptor declares `CAP_RENDER_EXTENT`. A zero
 * `default` means the core's native raster size.
 */
typedef struct AstraEmuRenderExtentSupport {
  struct AstraEmuExtent min;
  struct AstraEmuExtent max;
  struct AstraEmuExtent default_;
  uint32_t flags;
  uint32_t reserved;
} AstraEmuRenderExtentSupport;

/**
 * `display_name` is the emulator name and `family_name` the engine name shown
 * to users; both are 1..=64 UTF-8 bytes without control characters.
 */
typedef struct AstraEmuDescriptor {
  struct AstraEmuHeader header;
  struct AstraEmuBytes family_id;
  struct AstraEmuBytes plugin_id;
  struct AstraEmuBytes version;
  uint64_t capabilities;
  const struct AstraEmuBytes *formats;
  size_t format_count;
  const struct AstraEmuConfigField *configuration;
  size_t configuration_count;
  struct AstraEmuBytes display_name;
  struct AstraEmuBytes family_name;
  struct AstraEmuRenderExtentSupport render;
} AstraEmuDescriptor;

/**
 * Probe strings borrow module storage until the next ordinary module call.
 * Manager copies bounded metadata before making that call. NO_MATCH is normal.
 */
typedef struct AstraEmuProbeResult {
  struct AstraEmuBytes game_id;
  struct AstraEmuBytes format;
  uint32_t confidence_permyriad;
  uint32_t reserved;
} AstraEmuProbeResult;

typedef struct AstraEmuConfigEntry {
  struct AstraEmuBytes id;
  struct AstraEmuConfigValue value;
} AstraEmuConfigEntry;

typedef struct AstraEmuWindowState {
  uint32_t drawable_width;
  uint32_t drawable_height;
  double scale_factor;
  uint32_t focused;
  uint32_t suspended;
} AstraEmuWindowState;

typedef struct AstraEmuTextItem {
  uint64_t id;
  uint32_t kind;
  uint32_t reserved;
  struct AstraEmuBytes text;
} AstraEmuTextItem;

typedef struct AstraEmuTextRequest {
  uint64_t session_epoch;
  uint64_t context_epoch;
  uint64_t request_id;
  const struct AstraEmuTextItem *items;
  size_t item_count;
} AstraEmuTextRequest;

typedef struct AstraEmuTextRange {
  uint64_t id;
  size_t offset;
  size_t len;
} AstraEmuTextRange;

/**
 * Caller provides text and item storage. On CAPACITY, required capacities are
 * returned and the same request ID is retried without executing Luau/network
 * twice. Host retains exactly one result until acknowledge or cancellation.
 */
typedef struct AstraEmuTextResult {
  uint8_t *bytes;
  size_t byte_capacity;
  size_t byte_count;
  struct AstraEmuTextRange *items;
  size_t item_capacity;
  size_t item_count;
} AstraEmuTextResult;

typedef struct AstraEmuTextService {
  void *context;
  AstraEmuStatus (*request)(void*,
                            const struct AstraEmuTextRequest*,
                            struct AstraEmuTextResult*,
                            struct AstraEmuError*);
  AstraEmuStatus (*acknowledge)(void*, uint64_t, uint64_t, uint64_t);
  /**
   * Independent native cancellation, never queued behind the Luau VM.
   */
  void (*invalidate)(void*, uint64_t, uint64_t);
} AstraEmuTextService;

typedef struct AstraEmuWindowButton {
  uint32_t id;
  uint32_t reserved;
  struct AstraEmuBytes label;
} AstraEmuWindowButton;

/**
 * Typed bounded platform request; interpretation is selected by `kind`.
 * Window titles <=4096 bytes; modal caption/message/input <=65536 bytes.
 * Icons are <=256x256 tightly packed RGBA8. Modal buttons are <=16 items
 * with <=256-byte labels. MODAL_TEXT_INPUT distinguishes empty input from
 * a message-only dialog. Cancellation returns CANCELLED, not a button answer.
 * Modal buttons are explicit IDs; Host never invents an answer headlessly.
 * QUERY_STATE returns only fields the native platform can actually observe.
 * Focus/suspension must never be substituted for fullscreen/minimized state.
 * FULLSCREEN flags select one FULLSCREEN_* mode; exclusive width/height
 * request an exact physical display mode. Unsupported modes fail explicitly.
 * POINTER flags select independent properties; x/y use the logical stage.
 * SET_POSITION supplies both coordinates; SET_X/SET_Y preserve the other
 * coordinate. Do not combine SET_POSITION with the individual coordinate bits.
 * SIZE uses physical client pixels; POSITION uses physical screen coordinates.
 * These requests alter Host output O, never the core's raster R.
 */
typedef struct AstraEmuWindowRequest {
  uint64_t session_epoch;
  uint64_t context_epoch;
  uint64_t request_id;
  uint32_t kind;
  uint32_t flags;
  struct AstraEmuBytes text;
  struct AstraEmuBytes pixels;
  uint32_t width;
  uint32_t height;
  int32_t x;
  int32_t y;
  uint64_t autohide_ms;
  const struct AstraEmuWindowButton *buttons;
  size_t button_count;
  struct AstraEmuBytes caption;
  struct AstraEmuBytes initial_text;
} AstraEmuWindowRequest;

/**
 * Caller-owned output, initialized before request; service preserves data and
 * capacity and writes only initialized UTF-8 bytes and len. Input dialogs
 * require capacity >= MAX_MODAL_TEXT before displaying UI; insufficient
 * capacity returns CAPACITY without displaying or consuming a user answer.
 */
typedef struct AstraEmuTextBuffer {
  uint8_t *data;
  size_t capacity;
  size_t len;
} AstraEmuTextBuffer;

typedef struct AstraEmuWindowResult {
  uint32_t selected_button;
  uint32_t reserved;
  uint32_t valid_fields;
  uint32_t fullscreen;
  uint32_t width;
  uint32_t height;
  int32_t x;
  int32_t y;
  struct AstraEmuTextBuffer input;
  uint32_t visible;
  uint32_t minimized;
} AstraEmuWindowResult;

typedef struct AstraEmuWindowService {
  void *context;
  AstraEmuStatus (*request)(void*,
                            const struct AstraEmuWindowRequest*,
                            struct AstraEmuWindowResult*,
                            struct AstraEmuError*);
  void (*invalidate)(void*, uint64_t, uint64_t);
} AstraEmuWindowService;

/**
 * Host-owned cancellation exists before open returns a Session. It is required
 * even without text/window services; return 1 when this session epoch is
 * cancelled, 0 otherwise. The callback must be nonblocking and thread-safe.
 */
typedef struct AstraEmuCancellationService {
  void *context;
  uint32_t (*is_cancelled)(void*, uint64_t);
} AstraEmuCancellationService;

/**
 * Services copied during open. Cancellation is required; text/window are
 * optional. Context objects stay alive until failed open or close returns.
 * Host cancellation also wakes its blocked services; core checks cancellation
 * between initialization stages and native work, including before open returns.
 */
typedef struct AstraEmuHostServices {
  struct AstraEmuHeader header;
  struct AstraEmuTextService text;
  struct AstraEmuWindowService window;
  struct AstraEmuCancellationService cancellation;
} AstraEmuHostServices;

typedef void *AstraEmuModuleOwner;

typedef struct AstraEmuOpenRequest {
  struct AstraEmuHeader header;
  struct AstraEmuBytes game_path;
  uint64_t session_epoch;
  const struct AstraEmuConfigEntry *configuration;
  size_t configuration_count;
  struct AstraEmuWindowState window;
  struct AstraEmuHostServices host;
  /**
   * Opaque owner from the negotiated module-owner extension, or null when
   * the core has no cross-session owner extension.
   */
  AstraEmuModuleOwner module_owner;
  /**
   * 0x0 selects the core default. Nonzero values require
   * `CAP_RENDER_EXTENT` and must lie within the declared min/max; otherwise
   * open fails with INVALID_ARGUMENT.
   */
  struct AstraEmuExtent render_extent;
} AstraEmuOpenRequest;

typedef void *AstraEmuSession;

typedef struct AstraEmuSurfaceInfo {
  uint32_t kind;
  uint32_t pixel_format;
  uint32_t color;
  uint32_t alpha;
  uint32_t rows;
  struct AstraEmuExtent logical;
  struct AstraEmuExtent raster;
  uint64_t stride;
} AstraEmuSurfaceInfo;

typedef struct AstraEmuOpenResult {
  AstraEmuSession session;
  uint64_t context_epoch;
  uint64_t output_epoch;
  struct AstraEmuSurfaceInfo surface;
} AstraEmuOpenResult;

typedef struct AstraEmuFrameIdentity {
  uint64_t session_epoch;
  uint64_t output_epoch;
  uint64_t generation;
} AstraEmuFrameIdentity;

/**
 * Pointer coordinates are pixels in `displayed` frame, not window or logical
 * stage coordinates. Core applies its own frame-to-game/UI mapping once.
 * Physical keys use winit code names (KeyA, Digit1, Equal, Backquote, Enter).
 * Logical keys contain actual UTF-8 characters or named keys (Enter, Escape),
 * never Debug wrappers/control characters. Wheel x/y are fractional line
 * detents, not pixels. pressed/repeat and window booleans are exactly 0 or 1.
 */
typedef struct AstraEmuInputEvent {
  uint32_t kind;
  uint32_t pressed;
  uint32_t repeat;
  uint32_t button;
  struct AstraEmuBytes physical_key;
  struct AstraEmuBytes logical_key;
  double x;
  double y;
  struct AstraEmuFrameIdentity displayed;
  struct AstraEmuWindowState window;
} AstraEmuInputEvent;

typedef struct AstraEmuSessionState {
  uint32_t state;
  uint32_t flags;
  uint64_t context_epoch;
  uint64_t output_epoch;
  struct AstraEmuError error;
} AstraEmuSessionState;

/**
 * One independently owned immutable lease. Return it exactly once, after the
 * actual upload API no longer reads the source. The CPU owner and release
 * callback remain valid after session close; release must never access a VM,
 * device, window, session or host callback. No pixel scans are required.
 */
typedef struct AstraEmuCpuLease {
  struct AstraEmuBytes pixels;
  void *owner;
  void (*release)(void*);
} AstraEmuCpuLease;

typedef struct AstraEmuFrameSurface {
  uint32_t state;
  uint32_t reserved;
  struct AstraEmuFrameIdentity identity;
  uint64_t timestamp_ns;
  struct AstraEmuSurfaceInfo info;
  /**
   * Populated only for a Ready frame.
   */
  struct AstraEmuCpuLease cpu;
} AstraEmuFrameSurface;

/**
 * Ordinary calls are serialized by the Host. The core schedules itself; no
 * elapsed time or advance callback exists. Session panic poisons gameplay but
 * cancel/close must still perform all reachable cleanup. `close` consumes the
 * handle on every outcome and returns only after all workers and callbacks end.
 * A failed `open` leaves a null session and performs the same cleanup itself.
 * Before close, Host prevents new cancel calls and waits for in-flight cancel
 * calls to return. No call may use the consumed handle during or after close.
 * Host publishes a successful open handle under the same synchronization as
 * cancellation; if cancellation won, it cancels/closes instead of publishing.
 */
typedef struct AstraEmuCoreApi {
  struct AstraEmuHeader header;
  AstraEmuStatus (*initialize)(const struct AstraEmuDiagnostics*, struct AstraEmuError*);
  AstraEmuStatus (*describe)(struct AstraEmuDescriptor*, struct AstraEmuError*);
  AstraEmuStatus (*probe)(struct AstraEmuBytes, struct AstraEmuProbeResult*, struct AstraEmuError*);
  AstraEmuStatus (*open)(const struct AstraEmuOpenRequest*,
                         struct AstraEmuOpenResult*,
                         struct AstraEmuError*);
  AstraEmuStatus (*input)(AstraEmuSession,
                          const struct AstraEmuInputEvent*,
                          size_t,
                          struct AstraEmuError*);
  AstraEmuStatus (*control)(AstraEmuSession, uint32_t, struct AstraEmuError*);
  AstraEmuStatus (*state)(AstraEmuSession, struct AstraEmuSessionState*, struct AstraEmuError*);
  AstraEmuStatus (*frame)(AstraEmuSession,
                          struct AstraEmuFrameIdentity,
                          struct AstraEmuFrameSurface*,
                          struct AstraEmuError*);
  void (*cancel)(AstraEmuSession);
  AstraEmuStatus (*close)(AstraEmuSession, struct AstraEmuError*);
  /**
   * Returns a library-lifetime versioned C table, or UNSUPPORTED with null
   * output. Backend protocols are defined only with a real implementation.
   */
  AstraEmuStatus (*extension)(struct AstraEmuBytes, uint32_t, const void**);
} AstraEmuCoreApi;

/**
 * Query writes null on unsupported version, otherwise returns one immutable
 * module table. It performs no IO, starts no workers and installs no logger.
 */
typedef AstraEmuStatus (*AstraEmuQuery)(uint32_t, const struct AstraEmuCoreApi**);

/**
 * Optional ownership extension for real cross-session module resources, e.g.
 * Musica's explicit graphics Instance owner. A Host core instance creates one
 * owner after diagnostic initialization and retains it through all its sessions.
 * Other cores report UNSUPPORTED rather than constructing a dummy owner.
 *
 * create does not start a game. On error it returns null after cleanup; Host
 * also destroys a non-null failed result defensively. destroy consumes the owner
 * on every outcome, only after sessions close and callbacks finish. Both calls
 * run on the module owner thread. Libraries remain resident after destruction.
 */
typedef struct AstraEmuModuleOwnerApi {
  struct AstraEmuHeader header;
  AstraEmuStatus (*create)(AstraEmuModuleOwner*, struct AstraEmuError*);
  AstraEmuStatus (*destroy)(AstraEmuModuleOwner, struct AstraEmuError*);
} AstraEmuModuleOwnerApi;

/**
 * One Host-invoked session control. `control` is unique and
 * `>= CONTROL_CORE_BASE`. `label` is library-lifetime UTF-8.
 */
typedef struct AstraEmuSessionMenuItem {
  uint32_t control;
  uint32_t icon;
  struct AstraEmuBytes label;
} AstraEmuSessionMenuItem;

/**
 * Core-defined in-session menu. The table and labels are library-lifetime,
 * immutable, UTF-8. Invoking an item means the Host calls
 * `CoreApi.control(session, item.control)`. Controls are `>= CONTROL_CORE_BASE`
 * and unique. A core returns `INVALID_STATE` when the action is not possible
 * now; this is not a session failure.
 */
typedef struct AstraEmuSessionMenuApi {
  struct AstraEmuHeader header;
  const struct AstraEmuSessionMenuItem *items;
  size_t count;
} AstraEmuSessionMenuApi;

#define AstraEmuOK 0

#define AstraEmuUNSUPPORTED 1

#define AstraEmuINVALID_ARGUMENT 2

#define AstraEmuINVALID_STATE 3

#define AstraEmuCAPACITY 4

#define AstraEmuCANCELLED 5

#define AstraEmuCORE_ERROR 6

#define AstraEmuPANICKED 7

#define AstraEmuNO_MATCH 8

#define AstraEmuSTALE_CONTEXT 9

#endif  /* ASTRA_EMU_ABI_H */
