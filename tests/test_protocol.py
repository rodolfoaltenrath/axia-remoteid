import base64
import hashlib
import unittest
from tests.reference.remoteid_protocol import (
    ProtocolError, flatten_values, local_authorization, open_session_payload,
    hash_signature_payload, decode_signature_response, reference_from_key_name,
    SESSION_CACHE_LIFETIME_SECONDS,
)


class CanonicalInputTests(unittest.TestCase):
    def test_observed_order_and_boolean_semantics(self):
        body = {'z': 'fim', 'push': True, 'a': 'inicio', 'ignore': False, 'null': None}
        self.assertEqual(flatten_values(body), b'iniciopushfim')

    def test_nested_hash_array_and_integer_id(self):
        body = {'sessionToken': 'token', 'hashArray': [{'id': 0, 'hash': 'HASH'}], 'algorithm': 'SHA256'}
        self.assertEqual(flatten_values(body), b'SHA256HASH0token')

    def test_unicode_utf8_and_absence_of_json_escaping(self):
        self.assertEqual(flatten_values({'a': 'João\n"'}), 'João\n"'.encode('utf-8'))

    def test_scalar_array_is_not_assumed_compatible(self):
        with self.assertRaises(ProtocolError):
            flatten_values({'array': [1, 2]})

    def test_unmodeled_numeric_values_are_rejected(self):
        for value in (1.25, 2**63):
            with self.subTest(value=value), self.assertRaises(ProtocolError):
                flatten_values({'value': value})


class PayloadTests(unittest.TestCase):
    def test_pin_otp_preserve_leading_zeroes(self):
        body = open_session_payload(desktop_code='lab', pin='0001', otp='001234', push=False,
                                    application_name='test', serial_number='serial', issue='issuer')
        self.assertEqual(body['otp'], '001234')
        self.assertEqual(body['pin'], '0001')
        self.assertEqual(set(body), {'desktopCode', 'pin', 'otp', 'push', 'nomeAplicacaoDesktop', 'serialNumber', 'issue'})

    def test_hash_is_digest_not_full_document_or_digestinfo(self):
        digest = hashlib.sha256(b'offline laboratory input').digest()
        body = hash_signature_payload(desktop_code='lab', session_token='synthetic',
            serial_number='serial', issue='issuer', algorithm='SHA256', digest=digest)
        self.assertEqual(base64.b64decode(body['hashArray'][0]['hash']), digest)
        with self.assertRaises(ProtocolError):
            hash_signature_payload(desktop_code='lab', session_token='synthetic',
                serial_number='serial', issue='issuer', algorithm='SHA256', digest=b'wrong size')

    def test_response_error_and_corruption_rejected(self):
        bad = [{'status': False}, {'status': True, 'idArray': []},
               {'status': True, 'idArray': [{'status': True, 'signatureBase64': '%%%'}]},
               {'status': True, 'idArray': [{'status': False, 'signatureBase64': 'YQ=='}]},
               {'status': True, 'idArray': [{'status': True, 'signatureBase64': 'YQ=='},
                                           {'status': True, 'signatureBase64': 'YQ=='}]}]
        for body in bad:
            with self.subTest(body=body), self.assertRaises(ProtocolError):
                decode_signature_response(body)

    def test_success_response_preserves_signature_bytes(self):
        signature = bytes(range(256))
        body = {'status': True, 'idArray': [{'status': True, 'id': 0,
            'signatureBase64': base64.b64encode(signature).decode('ascii')}]}
        self.assertEqual(decode_signature_response(body), signature)
        # The original provider ignores the result ID, which need not echo 0.
        for returned_id in (0, 9, '0', 'lab-opaque-id', None):
            with self.subTest(returned_id=returned_id):
                body['idArray'][0]['id'] = returned_id
                self.assertEqual(decode_signature_response(body), signature)
        del body['idArray'][0]['id']
        self.assertEqual(decode_signature_response(body), signature)


class LocalAuthenticationTests(unittest.TestCase):
    def test_rsa_sha256_header_verifies_and_payload_change_fails(self):
        from cryptography.exceptions import InvalidSignature
        from cryptography.hazmat.primitives import hashes
        from cryptography.hazmat.primitives.asymmetric import padding, rsa
        key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
        body = {'momento': '1700000000', 'desktopCode': 'lab'}
        header = local_authorization(body, key)
        self.assertTrue(header.startswith('Bearer '))
        signature = base64.b64decode(header.removeprefix('Bearer '))
        self.assertEqual(len(signature), 256)
        # Verify the RSA encoded message independently of cryptography.verify.
        numbers = key.public_key().public_numbers()
        encoded = pow(int.from_bytes(signature, 'big'), numbers.e, numbers.n).to_bytes(256, 'big')
        digest_info = bytes.fromhex('3031300d060960864801650304020105000420') + hashlib.sha256(flatten_values(body)).digest()
        self.assertEqual(encoded, b'\x00\x01' + b'\xff' * (256 - len(digest_info) - 3) + b'\x00' + digest_info)
        key.public_key().verify(signature, flatten_values(body), padding.PKCS1v15(), hashes.SHA256())
        with self.assertRaises(InvalidSignature):
            key.public_key().verify(signature, flatten_values({**body, 'momento': '1700000001'}),
                                    padding.PKCS1v15(), hashes.SHA256())


if __name__ == '__main__':
    unittest.main()


class KeyNameReference(unittest.TestCase):
    def test_serial_comes_first_and_issue_second(self):
        self.assertEqual(
            reference_from_key_name('0A1B2C3D;AC Certisign RFB G5'),
            ('0A1B2C3D', 'AC Certisign RFB G5'))

    def test_separator_runs_are_compressed(self):
        self.assertEqual(reference_from_key_name('SERIE;;;EMISSOR'),
                         ('SERIE', 'EMISSOR'))

    def test_rejects_missing_parts_and_non_ascii(self):
        for value in ('semseparador', 'a;b;c', '', ';', 'SERIE;', 'SERIE;EMISSÃO'):
            with self.subTest(value=value):
                with self.assertRaises(ProtocolError):
                    reference_from_key_name(value)

    def test_rust_and_python_agree_on_cache_lifetime(self):
        self.assertEqual(SESSION_CACHE_LIFETIME_SECONDS, 1800)
