"""Generate PUBLIC laboratory fixtures for Python/Rust interoperability tests.

The embedded private key is freshly generated for tests, belongs to no account,
and must never be used as a real DesktopID installation identity.
"""
import base64
import datetime
import json
from pathlib import Path

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import padding, rsa, utils
from cryptography.x509.oid import NameOID

from tests.reference.remoteid_protocol import flatten_values, local_authorization


def generate():
    key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, 'DesktopID laboratory only')])
    certificate = (x509.CertificateBuilder().subject_name(name).issuer_name(name)
        .public_key(key.public_key()).serial_number(123456)
        .not_valid_before(datetime.datetime(2020, 1, 1, tzinfo=datetime.timezone.utc))
        .not_valid_after(datetime.datetime(2040, 1, 1, tzinfo=datetime.timezone.utc))
        .sign(key, hashes.SHA256()))
    bodies = [
        {'z': 'fim', 'push': True, 'a': 'inicio', 'ignore': False, 'null': None},
        {'sessionToken': 'token', 'hashArray': [{'id': 0, 'hash': 'HASH'}], 'algorithm': 'SHA256'},
        {'a': 'João\n"', 'nested': {'z': -9223372036854775808, 'a': 9223372036854775807}},
        {'desktopCode': 'lab', 'pin': '0001', 'otp': '001234', 'push': False,
         'nomeAplicacaoDesktop': 'Teste', 'issue': 'issuer', 'serialNumber': 'serial'},
        {'items': [{'x': 'one'}, {'x': 'two', 'active': True}], 'empty': {}, 'ignored': []},
    ]
    document = b'DesktopID offline Rust/Python signing interoperability\n'
    signatures = {}
    for label, algorithm in [('SHA1', hashes.SHA1()), ('SHA256', hashes.SHA256()),
                             ('SHA384', hashes.SHA384()), ('SHA512', hashes.SHA512())]:
        digest = hashes.Hash(algorithm)
        digest.update(document)
        signatures[label] = base64.b64encode(key.sign(digest.finalize(), padding.PKCS1v15(),
                                                      utils.Prehashed(algorithm))).decode()
    result = {
        'laboratory_only': True,
        'private_key_pem': key.private_bytes(serialization.Encoding.PEM,
            serialization.PrivateFormat.PKCS8, serialization.NoEncryption()).decode(),
        'certificate_pem': certificate.public_bytes(serialization.Encoding.PEM).decode(),
        'document_base64': base64.b64encode(document).decode(),
        'signatures': signatures,
        'vectors': [{'body': body, 'flattened_base64': base64.b64encode(flatten_values(body)).decode(),
                     'authorization': local_authorization(body, key)} for body in bodies],
    }
    destination = Path(__file__).resolve().parents[1]/'crates/desktopid-client/tests/fixtures/python-vectors.json'
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(json.dumps(result, indent=2, ensure_ascii=False)+'\n', encoding='utf-8')
    print(f'Public laboratory vectors written: {destination}')


if __name__ == '__main__':
    generate()
