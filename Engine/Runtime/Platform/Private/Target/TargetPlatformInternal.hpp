#pragma once

#include <Astra/Platform/Platform.hpp>

namespace Astra::Platform {

[[nodiscard]] TargetPlatformDesc WindowsTarget();
[[nodiscard]] TargetPlatformDesc LinuxTarget();
[[nodiscard]] TargetPlatformDesc MacOsX64Target();
[[nodiscard]] TargetPlatformDesc MacOsArm64Target();
[[nodiscard]] TargetPlatformDesc IosTarget();
[[nodiscard]] TargetPlatformDesc AndroidTarget();
[[nodiscard]] TargetPlatformDesc WebTarget();
[[nodiscard]] TargetPlatformDesc HeadlessTarget();

} // namespace Astra::Platform
