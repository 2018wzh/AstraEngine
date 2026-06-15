#include <Astra/ModuleRuntime/ModuleAbi.h>

extern "C" ASTRA_MODULE_EXPORT int astra_not_the_entrypoint() {
    return 1;
}
