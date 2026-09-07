"""Source hygiene for every registered production UI scenario and QA helper."""

import unittest

from test_qa_runner import CURVE, RUNNER, SCRIPTS


class QaSourceContractTests(unittest.TestCase):
    def test_suite_files_do_not_use_removed_project_fixture_or_ambiguous_editor_name(self):
        for suite in RUNNER.suite_specs("full"):
            source = (SCRIPTS / suite.script).read_text(encoding="utf-8")
            self.assertNotIn("retired_fixture", source)
            self.assertNotIn("qa_project_graph_base", source)
        self.assertEqual(CURVE.FIXTURE, "authoring_e2e")

    def test_every_active_qa_file_stays_below_one_thousand_lines(self):
        files = [SCRIPTS / "qa-runner.py"]
        files.extend(SCRIPTS / suite.script for suite in RUNNER.suite_specs("full"))
        files.extend(SCRIPTS.glob("qa_*.py"))
        files.extend(SCRIPTS.glob("test_qa*.py"))
        files.append(SCRIPTS / "qa-particle-persistence-e2e.py")
        for path in files:
            with self.subTest(path=path.name):
                self.assertLess(len(path.read_text(encoding="utf-8").splitlines()), 1000)
