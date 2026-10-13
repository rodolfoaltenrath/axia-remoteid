"""Recovered RemoteID request model. No networking or account access.

Evidence: macOS provider 2.2.0.1, flattenJsonValues @0x11fab,
produceSignatureForJson @0x121fa, openSession @0x60ed2, signHash @0x61c50.
The model deliberately accepts only the JSON shapes used by these requests.
It has not been checked against a live RemoteID account.
"""
import base64
from typing import Any


class ProtocolError(ValueError):
    pass


def flatten_values(body: dict[str, Any]) -> bytes:
    """Concatenate sorted object values, reproducing the observed signature input.

    Strings are unchanged; true contributes its field name; false/null contribute
    nothing; integer values use decimal; nested objects recurse; arrays contain
    objects. Floating-point and scalar array behavior is intentionally unmodeled.
    This is NOT JSON canonicalization as used by JCS, JWT, or ordinary OAuth.
    """
    if not isinstance(body, dict) or any(not isinstance(k, str) for k in body):
        raise ProtocolError('A JSON object with string field names is required')
    parts = []
    for key in sorted(body):
        value = body[key]
        if isinstance(value, str):
            parts.append(value.encode('utf-8'))
        elif value is True:
            parts.append(key.encode('utf-8'))
        elif value is False or value is None:
            continue
        elif isinstance(value, int):
            if not -(2**63) <= value < 2**63:
                raise ProtocolError('Integer outside the observed signed int64 range')
            parts.append(str(value).encode('ascii'))
        elif isinstance(value, dict):
            parts.append(flatten_values(value))
        elif isinstance(value, list):
            for item in value:
                parts.append(flatten_values(item))
        else:
            raise ProtocolError('Unmodeled JSON value: floating point or unsupported type')
    return b''.join(parts)


def local_authorization(body: dict[str, Any], private_key) -> str:
    """Sign with a lab RSA key, separate from the user's cloud certificate key.

    Original: RSA-2048 key, SHA256 hash and PKCS#1 v1.5 padding.
    This function neither creates an installation nor sends the generated header.
    """
    from cryptography.hazmat.primitives import hashes
    from cryptography.hazmat.primitives.asymmetric import padding, rsa
    if not isinstance(private_key, rsa.RSAPrivateKey) or private_key.key_size < 2048:
        raise ProtocolError('An RSA private key of at least 2048 bits is required')
    signature = private_key.sign(flatten_values(body), padding.PKCS1v15(), hashes.SHA256())
    return 'Bearer ' + base64.b64encode(signature).decode('ascii')


def open_session_payload(*, desktop_code: str, pin: str, otp: str, push: bool,
                         application_name: str, serial_number: str, issue: str) -> dict:
    if not isinstance(push, bool):
        raise ProtocolError('push must be a JSON boolean')
    body = {'desktopCode': desktop_code, 'pin': pin, 'otp': otp, 'push': push,
            'nomeAplicacaoDesktop': application_name, 'serialNumber': serial_number,
            'issue': issue}
    flatten_values(body)
    return body


def hash_signature_payload(*, desktop_code: str, session_token: str, serial_number: str,
                           issue: str, algorithm: str, digest: bytes) -> dict:
    expected = {'SHA1': 20, 'SHA256': 32, 'SHA384': 48, 'SHA512': 64}
    if algorithm not in expected or not isinstance(digest, bytes) or len(digest) != expected[algorithm]:
        raise ProtocolError('Expected a raw digest with the declared algorithm length')
    body = {'desktopCode': desktop_code, 'sessionToken': session_token,
            'serialNumber': serial_number, 'issue': issue, 'algorithm': algorithm,
            'hashArray': [{'id': 0, 'hash': base64.b64encode(digest).decode('ascii')}]}
    flatten_values(body)
    return body


def decode_signature_response(response: dict) -> bytes:
    """Decode a single result. Caller must verify its digest and certificate.

    Original signHash @0x62174..0x621f2 does not inspect the response id.
    Do not assume that it echoes the numeric request id (which remains 0).
    """
    if response.get('status') is not True:
        raise ProtocolError('Server did not report a successful signature')
    items = response.get('idArray')
    if not isinstance(items, list) or len(items) != 1 or not isinstance(items[0], dict):
        raise ProtocolError('Expected exactly one signature result')
    item = items[0]
    if item.get('status') is not True:
        raise ProtocolError('Individual signature result failed')
    value = item.get('signatureBase64')
    if not isinstance(value, str):
        raise ProtocolError('Missing signatureBase64')
    try:
        result = base64.b64decode(value, validate=True)
    except (ValueError, base64.binascii.Error) as error:
        raise ProtocolError('Invalid Base64 signature') from error
    if not result:
        raise ProtocolError('Empty signature')
    return result


SESSION_CACHE_LIFETIME_SECONDS = 30 * 60
"""Client-side session token cache lifetime used by the original provider.

TokenStore::addToken @0x4b042 stores each token under the map key
``(system_clock::now() + 1_800_000_000us) // 100_000us``, the expiry instant in
100ms units, and whenTokenFound @0x4b180 erases every entry below
``now // 100_000us`` before the lookup. The service may expire a token sooner.
"""


def reference_from_key_name(key_name: str) -> tuple[str, str]:
    """Split a wallet keyName into the protocol (serial_number, issue) pair.

    openSession @0x60ed2 and signHash @0x61c50 both run
    boost::algorithm::split(parts, keyName, is_any_of(";"), token_compress_on)
    and assign parts[0] to serialNumber and parts[1] to issue. The serial comes
    first; this is not the order of the X.509 display fields, so these values
    must never be derived from the certificate.

    Both call sites split a std::wstring into std::vector<std::string>, which
    narrows every character to char. Non-ASCII input would be mangled by the
    original, so it is rejected here rather than guessed. Empty segments are
    dropped and an exact pair is required, which rejects the ambiguous cases.
    """
    if not isinstance(key_name, str):
        raise ProtocolError('keyName must be a string')
    if not key_name.isascii():
        raise ProtocolError('keyName with non-ASCII characters has no confirmed representation')
    parts = [part for part in key_name.split(';') if part]
    if len(parts) != 2:
        raise ProtocolError(f'keyName must hold exactly serial and issuer separated by ";", got {len(parts)}')
    return parts[0], parts[1]
