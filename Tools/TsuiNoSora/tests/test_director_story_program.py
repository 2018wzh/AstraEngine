import unittest

from director_story_program import DirectorStoryProgramError, _compile_statements


def token(kind, value):
    return {"kind": kind, "value": value}


def call(name, symbol=None, number=None):
    values = [token("identifier", name), token("punctuation", "(")]
    if symbol is not None:
        values.append(token("symbol", symbol))
    if number is not None:
        if symbol is not None:
            values.append(token("punctuation", ","))
        values.append(token("number", str(number)))
    values.append(token("punctuation", ")"))
    return values


class DirectorStoryProgramTests(unittest.TestCase):
    def test_compiles_nested_flag_branch_and_mutation(self):
        condition = call("tgetdayflag", "#route") + [token("operator", "=")] + [token("number", "0")]
        statements = [
            {"kind": "if_begin", "condition": condition},
            {"kind": "command", "expression": call("tsetdayflag", "#route", 1)},
            {"kind": "go", "expression": call("label")[:-1] + [token("string", "A"), token("punctuation", ")")]},
            {"kind": "else"},
            {"kind": "go", "expression": call("label")[:-1] + [token("string", "B"), token("punctuation", ")")]},
            {"kind": "if_end"},
        ]

        program = _compile_statements(statements, {"A": "state.a", "B": "state.b"})

        self.assertEqual(program[0]["kind"], "if")
        self.assertEqual(program[0]["condition"]["variable"]["path"], "project.day.route")
        self.assertEqual(program[0]["then"][0]["kind"], "set_variable")
        self.assertEqual(program[0]["then"][1]["target"], "state.a")
        self.assertEqual(program[0]["else"][0]["target"], "state.b")

    def test_compiles_case_and_selector_effects(self):
        statements = [
            {"kind": "command", "expression": [token("identifier", "gselector1"), token("identifier", "mdisableitem"), token("number", "2")]},
            {"kind": "case_begin", "expression": call("tgetglobalflag", "#mode")},
            {"kind": "case_label", "value": [token("number", "0")]},
            {"kind": "go", "expression": call("label")[:-1] + [token("string", "A"), token("punctuation", ")")]},
            {"kind": "case_otherwise"},
            {"kind": "go", "expression": call("label")[:-1] + [token("string", "B"), token("punctuation", ")")]},
            {"kind": "case_end"},
        ]

        program = _compile_statements(statements, {"A": "state.a", "B": "state.b"})

        self.assertEqual(program[0]["kind"], "selector_set_enabled")
        self.assertFalse(program[0]["enabled"])
        self.assertEqual(program[1]["kind"], "case")
        self.assertEqual(program[1]["variable"]["path"], "global.mode")

    def test_unresolved_goto_is_blocking(self):
        statements = [
            {"kind": "go", "expression": call("label")[:-1] + [token("string", "missing"), token("punctuation", ")")]}
        ]
        with self.assertRaises(DirectorStoryProgramError):
            _compile_statements(statements, {})


if __name__ == "__main__":
    unittest.main()
