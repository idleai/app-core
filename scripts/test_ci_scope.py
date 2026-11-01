import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import ci_scope
from ci_scope import BROWSERS, select


class ScopeTests(unittest.TestCase):
    def test_docs_retain_only_the_main_checks(self):
        plan = select(["README.md"])
        self.assertFalse(plan["windows_dependencies"])
        self.assertFalse(plan["native_bindings"])
        self.assertFalse(plan["consumers"])
        self.assertEqual(plan["browser"], [])

    def test_shared_dependency_changes_require_repository_checks(self):
        plan = select(["Cargo.lock"])
        self.assertTrue(plan["windows_dependencies"])
        self.assertTrue(plan["native_bindings"])
        self.assertFalse(plan["consumers"])
        self.assertEqual(plan["browser"], list(BROWSERS))

    def test_dependency_script_changes_keep_windows_validation(self):
        self.assertTrue(select(["scripts/release_dependencies.py"])["windows_dependencies"])

    def test_native_ffi_changes_select_native_checks(self):
        plan = select(["crates/app-core-bindings/src/ffi.rs"])
        self.assertTrue(plan["native_bindings"])
        self.assertFalse(plan["consumers"])

    def test_core_payload_changes_check_native_types_without_a_consumer_release(self):
        plan = select(["crates/app-core/src/history/model.rs"])
        self.assertFalse(plan["consumers"])
        self.assertTrue(plan["native_bindings"])

    def test_shared_web_components_run_all_browser_suites(self):
        self.assertEqual(select(["crates/web-ui/src/lib.rs"])["browser"], list(BROWSERS))

    def test_shared_geometry_runs_browser_suites(self):
        self.assertEqual(select(["crates/history-geometry/src/lib.rs"])["browser"], list(BROWSERS))

    def test_fixture_change_selects_one_browser_suite(self):
        self.assertEqual(select(["crates/web-ui/examples/sessions.rs"])["browser"], ["sessions"])

    def test_css_selects_every_fixture_that_copies_it(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "scripts").mkdir()
            for suite in ("history", "navigation"):
                (root / f"scripts/build-{suite}.sh").write_text("cp crates/web-ui/assets/history.css out/\n")
            plan = select(["crates/web-ui/assets/history.css"], root=root)
            self.assertEqual(plan["browser"], ["history", "navigation"])

    def test_release_and_integration_runs_are_full(self):
        self.assertEqual(select([], full=True)["browser"], list(BROWSERS))

    def test_pr_defers_bindings_until_full_validation(self):
        paths = ["crates/app-core/src/history/model.rs"]
        quick = select(paths, quick=True)
        self.assertFalse(quick["native_bindings"])
        self.assertFalse(quick["consumers"])
        complete = select(paths, quick=True, full=True)
        self.assertTrue(complete["native_bindings"])
        self.assertFalse(complete["consumers"])

    def test_released_consumers_are_selected_only_when_requested(self):
        for paths, full in ((["Cargo.lock"], False), ([], True)):
            with self.subTest(paths=paths, full=full):
                self.assertFalse(select(paths, full=full)["consumers"])
                self.assertTrue(select(paths, full=full, check_consumers=True)["consumers"])

    def test_requested_consumer_selection_reaches_workflow_outputs(self):
        for requested in ("false", "true"):
            with self.subTest(requested=requested), tempfile.TemporaryDirectory() as directory:
                output = Path(directory) / "outputs"
                with patch.dict(os.environ, {"GITHUB_OUTPUT": str(output), "FULL_CI": "true",
                                             "BASE_SHA": "", "PR_CI": "false",
                                             "CHECK_RELEASED_CONSUMERS": requested}):
                    ci_scope.main()
                values = dict(line.split("=", 1) for line in output.read_text().splitlines())
                self.assertEqual(values["consumers"], requested)
                self.assertEqual(values["native_bindings"], "true")

    def test_consumer_opt_in_is_restricted_to_manual_workflows(self):
        workflow = (Path(__file__).resolve().parent.parent / ".github/workflows/ci.yml").read_text()
        dispatch = workflow.split("  workflow_dispatch:\n", 1)[1].split("  workflow_call:\n", 1)[0]
        consumer = dispatch.split("      check_consumers:\n", 1)[1]
        self.assertIn("        type: boolean\n        default: false\n", consumer)
        self.assertIn("CHECK_RELEASED_CONSUMERS: ${{ github.event_name == 'workflow_dispatch' && inputs.check_consumers }}", workflow)


if __name__ == "__main__":
    unittest.main()
