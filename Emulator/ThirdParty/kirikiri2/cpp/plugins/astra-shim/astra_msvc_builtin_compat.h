// MSVC has no __builtin_* shorthands; map the ones the vendored sources use.
#pragma once

#if defined(_MSC_VER) && !defined(__clang__)
#include <cstring>
#include <cstdlib>
#define __builtin_memcpy memcpy
#define __builtin_memset memset
#define __builtin_trap() abort()
#endif
