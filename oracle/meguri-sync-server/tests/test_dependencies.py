# SPDX-License-Identifier: AGPL-3.0-or-later
import unittest
from audit_deps import license_matches


class DependencyLicenses(unittest.TestCase):
    def test_orjson_review_keeps_its_mpl_term(self):
        self.assertTrue(license_matches('orjson', 'MPL-2.0 AND (Apache-2.0 OR MIT)'))
        self.assertFalse(license_matches('orjson', 'Apache-2.0 OR MIT'))

    def test_unknown_license_or_package_fails(self):
        self.assertFalse(license_matches('jsonschema', 'unknown'))
        self.assertFalse(license_matches('unreviewed-package', 'MIT'))
