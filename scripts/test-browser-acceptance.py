#!/usr/bin/env python3
"""Pure boundary tests; no containers, app server, browser or network are started."""
import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("browser_acceptance", Path(__file__).with_name("browser-acceptance.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class Boundaries(unittest.TestCase):
    def setUp(self):
        self.env = {"GITHUB_ACTIONS": "true", "RUNNER_OS": "Linux", "GITHUB_RUN_ID": "123", "GITHUB_RUN_ATTEMPT": "2"}
        self.project = "opentrack-browser-123-2-amd64"
        self.config = {"name": self.project, "volumes": {"database": {"name": self.project + "_database"}},
                       "services": {"app": {"ports": [{"host_ip": "127.0.0.1", "published": "18081", "target": 8080}]}, "database": {}}}

    def test_only_current_disposable_project(self):
        self.assertEqual(module.validate_target(self.project, "http://localhost:18081/", self.env), "http://localhost:18081")
        for project in ("opentrack", "opentrack-smoke-123-amd64", "opentrack-browser-123-1-amd64", "opentrack-browser-124-2-amd64", ""):
            with self.subTest(project=project), self.assertRaises(RuntimeError):
                module.validate_target(project, "http://localhost:18081", self.env)

    def test_no_local_or_non_loopback_runtime(self):
        for url in ("http://example.test:18081", "http://0.0.0.0:18081", "http://localhost:8080", "https://localhost:18081", "http://localhost:18081/path", "http://user@localhost:18081", "http://localhost:18081?target=elsewhere"):
            with self.subTest(url=url), self.assertRaises(RuntimeError):
                module.validate_target(self.project, url, self.env)
        for key in self.env:
            with self.subTest(key=key), self.assertRaises(RuntimeError):
                module.validate_target(self.project, "http://localhost:18081", {**self.env, key: ""})

    def test_compose_cannot_reach_user_volumes_or_ports(self):
        module.validate_compose(self.config, self.project)
        changes = [lambda c: c.update(name="opentrack"),
                   lambda c: c["volumes"]["database"].update(name="opentrack_database"),
                   lambda c: c["volumes"]["database"].update(external=True),
                   lambda c: c["services"]["app"]["ports"][0].update(host_ip="0.0.0.0"),
                   lambda c: c["services"]["database"].update(ports=[5432]),
                   lambda c: c["services"]["app"].update(volumes=[{"type": "bind", "source": "/tmp"}])]
        for change in changes:
            config = copy.deepcopy(self.config)
            change(config)
            with self.assertRaises(RuntimeError):
                module.validate_compose(config, self.project)


if __name__ == "__main__":
    unittest.main()
