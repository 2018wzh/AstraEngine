from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from Tools.AstraEMU import check_core_deps


def dumpbin_output(*dependencies, delay=()):
    lines = ["Microsoft (R) COFF/PE Dumper Version 14.44.35207.1", "",
             "Dump of file core.dll", "", "File Type: DLL", "",
             "  Image has the following dependencies:", ""]
    lines += [f"    {name}" for name in dependencies]
    if delay:
        lines += ["", "  Image has the following delay load dependencies:", ""]
        lines += [f"    {name}" for name in delay]
    lines += ["", "  Summary", "", "        1000 .data", "        2000 .rdata"]
    return "\n".join(lines) + "\n"


class CheckCoreDepsTests(unittest.TestCase):
    def setUp(self):
        self.temporary = TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.system = self.root / "System32"
        for name in ("KERNEL32.dll", "USER32.dll", "msvcp140.dll", "VCOMP140.DLL"):
            self.file("System32/" + name)
        self.core = self.root / "cores/astra_emu_example"
        self.file("cores/astra_emu_example/astra_emu_example.dll")
        self.file("cores/astra_emu_example/avcodec-62.dll")
        self.outputs = {}

    def file(self, relative):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"MZ")
        return path

    def dump(self, path):
        return self.outputs.get(path.name, dumpbin_output("KERNEL32.dll"))

    def check(self):
        return check_core_deps.violations([self.core], system_dir=self.system, dump=self.dump)

    def test_parser_reads_direct_and_delay_load_sections_only(self):
        text = dumpbin_output("avcodec-62.dll", "KERNEL32.dll", delay=("USER32.dll",))
        self.assertEqual(["avcodec-62.dll", "KERNEL32.dll", "USER32.dll"],
                         check_core_deps.parse_dependents(text))

    def test_allowed_dependency_classes_pass(self):
        self.outputs["astra_emu_example.dll"] = dumpbin_output(
            "AVCODEC-62.DLL", "VCRUNTIME140.dll", "vcruntime140_1.dll", "MSVCP140.dll",
            "api-ms-win-crt-runtime-l1-1-0.dll", "KERNEL32.dll", delay=("USER32.dll",))
        self.assertEqual([], self.check())

    def test_missing_dependency_is_reported(self):
        self.outputs["avcodec-62.dll"] = dumpbin_output("swresample-6.dll", "KERNEL32.dll")
        problems = self.check()
        self.assertEqual(1, len(problems))
        self.assertIn("avcodec-62.dll", problems[0])
        self.assertIn("swresample-6.dll", problems[0])

    def test_system_copy_of_redistributable_runtime_is_not_accepted(self):
        self.outputs["astra_emu_example.dll"] = dumpbin_output("vcomp140.dll")
        self.assertEqual(1, len(self.check()))
        self.file("cores/astra_emu_example/vcomp140.dll")
        self.assertEqual([], self.check())

    def test_nested_dlls_are_checked_but_only_top_level_files_resolve(self):
        self.file("cores/astra_emu_example/Resources/plugin.dll")
        self.outputs["plugin.dll"] = dumpbin_output("helper.dll")
        self.file("cores/astra_emu_example/Resources/helper.dll")
        self.assertEqual(1, len(self.check()))

    def test_directory_without_dll_is_reported(self):
        empty = self.root / "cores/astra_emu_empty"
        empty.mkdir(parents=True)
        self.assertEqual(1, len(check_core_deps.violations([empty], system_dir=self.system, dump=self.dump)))

    def test_closure_collects_transitive_app_local_dependencies_in_search_order(self):
        build = self.file("build/astra_emu_example.dll").parent
        self.file("build/avformat-62.dll")
        self.file("vcpkg/bin/avformat-62.dll")
        self.file("vcpkg/bin/avcodec-62.dll")
        self.file("vcpkg/bin/swresample-6.dll")
        self.file("vcpkg/bin/unused.dll")
        self.outputs["astra_emu_example.dll"] = dumpbin_output(
            "avformat-62.dll", "KERNEL32.dll", "VCRUNTIME140.dll", "api-ms-win-crt-heap-l1-1-0.dll",
            delay=("missing.dll",))
        self.outputs["avformat-62.dll"] = dumpbin_output("avcodec-62.dll", "KERNEL32.dll")
        self.outputs["avcodec-62.dll"] = dumpbin_output("swresample-6.dll", "avformat-62.dll")
        self.outputs["swresample-6.dll"] = dumpbin_output("KERNEL32.dll")
        found = check_core_deps.closure(build / "astra_emu_example.dll", [build, self.root / "vcpkg/bin"],
                                        dump=self.dump)
        self.assertEqual([self.root / "vcpkg/bin/avcodec-62.dll", build / "avformat-62.dll",
                          self.root / "vcpkg/bin/swresample-6.dll"], found)

    def test_closure_skips_package_root_runtime(self):
        build = self.file("build/astra_emu_example.dll").parent
        self.file("build/msvcp140.dll")
        self.outputs["astra_emu_example.dll"] = dumpbin_output("MSVCP140.dll")
        self.assertEqual([], check_core_deps.closure(build / "astra_emu_example.dll", [build], dump=self.dump))


if __name__ == "__main__":
    unittest.main()
