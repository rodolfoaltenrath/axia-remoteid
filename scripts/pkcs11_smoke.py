"""Native PKCS11 tests with public lab keys, a synthetic helper and no cloud access."""
import argparse
import base64
import ctypes as C
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
from cryptography import x509
from cryptography.hazmat.primitives import serialization

U = C.c_ulong
P = C.c_void_p
class Attribute(C.Structure):
    _fields_ = [('type', U), ('value', P), ('length', U)]
class Mechanism(C.Structure):
    _fields_ = [('type', U), ('parameter', P), ('length', U)]
class Version(C.Structure):
    _fields_ = [('major', C.c_ubyte), ('minor', C.c_ubyte)]
class FunctionList(C.Structure):
    _fields_ = [('version', Version), ('functions', P*68)]

def buffer(data):
    return C.create_string_buffer(data, max(1, len(data)))

def main(module, client, report):
    fixture = Path(__file__).resolve().parents[1]/'crates/desktopid-client/tests/fixtures/python-vectors.json'
    data = json.loads(fixture.read_text())
    document = base64.b64decode(data['document_base64'])
    certificate = x509.load_pem_x509_certificate(data['certificate_pem'].encode())
    checks = []
    with tempfile.TemporaryDirectory(prefix='desktopid-pkcs11-lab-') as temp:
        temp = Path(temp)
        state = temp/'identity'
        subprocess.run([client, '--state-dir', str(state), 'init', '--server', 'https://example.invalid', '--access-url', ''], check=True, capture_output=True)
        cfg = json.loads((state/'config.json').read_text())
        cfg['desktop_code'] = 'synthetic-lab-code'
        (state/'config.json').write_text(json.dumps(cfg))
        wallet = {'schema_version':1, 'public_key_sha256':cfg['public_key_sha256'], 'desktop_code':cfg['desktop_code'],
                  'certificados':[{'index':0, 'keyName':'SERIAL;ISSUER',
                                   'base64':base64.b64encode(certificate.public_bytes(serialization.Encoding.DER)).decode()}]}
        (state/'wallet.json').write_text(json.dumps(wallet))
        (state/'wallet.json').chmod(0o600)
        helper = temp/'fake-helper'
        counter = temp/'calls.jsonl'
        helper.write_text('''#!/usr/bin/python3
import base64, json, sys
from pathlib import Path
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import padding, utils
fixture=json.loads(Path(%r).read_text())
req=json.load(sys.stdin)
with Path(%r).open('a') as f: f.write(json.dumps({'algorithm':req['algorithm']})+'\\n')
key=serialization.load_pem_private_key(fixture['private_key_pem'].encode(),None)
algorithm={'SHA256':hashes.SHA256,'SHA384':hashes.SHA384,'SHA512':hashes.SHA512}[req['algorithm']]()
signature=key.sign(base64.b64decode(req['digest_base64']),padding.PKCS1v15(),utils.Prehashed(algorithm))
print(json.dumps({'schema_version':1,'signature_base64':base64.b64encode(signature).decode()}))
''' % (str(fixture), str(counter)))
        helper.chmod(0o700)
        synthetic_helper=helper.read_text()
        os.environ['DESKTOPID_STATE_DIR'] = str(state)
        os.environ['DESKTOPID_HELPER'] = str(helper)
        lib = C.CDLL(str(module))
        def call(name, types, *args, expected=0):
            fn = getattr(lib, name)
            fn.argtypes = types
            fn.restype = U
            rv = fn(*args)
            assert rv == expected, f'{name}: expected {expected:#x}, got {rv:#x}'
        call('C_GetSlotList', [C.c_ubyte,P,P], 1,None,C.byref(U()), expected=0x190)
        table = C.POINTER(FunctionList)()
        call('C_GetFunctionList',[P],C.byref(table))
        assert (table.contents.version.major,table.contents.version.minor)==(2,40)
        assert all(table.contents.functions)
        checks.append('v2.40-table-with-68-non-null-functions')
        call('C_Initialize',[P],None)
        call('C_GetFunctionStatus',[U],0,expected=0x51)
        call('C_CancelFunction',[U],0,expected=0x51)
        call('C_Initialize',[P],None,expected=0x191)
        slot_count=U()
        call('C_GetSlotList',[C.c_ubyte,P,P],1,None,C.byref(slot_count))
        assert slot_count.value==1
        slots=(U*1)()
        slot_count.value=0
        call('C_GetSlotList',[C.c_ubyte,P,P],1,slots,C.byref(slot_count),expected=0x150)
        call('C_GetSlotList',[C.c_ubyte,P,P],1,slots,C.byref(slot_count))
        assert slots[0]==8
        checks.append('initialization-and-slot-size-negotiation')
        sessions=[]
        for _ in range(2):
            session=U()
            call('C_OpenSession',[U,U,P,P,P],8,4,None,None,C.byref(session))
            sessions.append(session)
        s, other=sessions
        def find(session, cls=None):
            value=U(cls or 0)
            attr=Attribute(0,C.cast(C.byref(value),P),C.sizeof(U))
            call('C_FindObjectsInit',[U,P,U],session,C.byref(attr) if cls is not None else None,1 if cls is not None else 0)
            found=(U*16)(); count=U()
            call('C_FindObjects',[U,P,U,P],session,found,16,C.byref(count))
            result=list(found)[:count.value]
            call('C_FindObjects',[U,P,U,P],session,found,16,C.byref(count))
            assert count.value==0
            call('C_FindObjectsFinal',[U],session)
            return result
        assert len(find(s))==2
        assert find(s,3)==[]
        call('C_Login',[U,U,P,U],s,1,None,0)
        assert len(find(s))==3
        assert len(find(other,3))==1
        cert_handle=find(s,1)[0]; pub=find(s,2)[0]; private=find(s,3)[0]
        checks.append('public-inventory-and-token-wide-protected-path-login')
        def attr_value(obj, ty):
            attr=Attribute(ty,None,0)
            call('C_GetAttributeValue',[U,U,P,U],s,obj,C.byref(attr),1)
            raw=C.create_string_buffer(attr.length)
            attr.value=C.cast(raw,P)
            call('C_GetAttributeValue',[U,U,P,U],s,obj,C.byref(attr),1)
            return raw.raw
        assert attr_value(cert_handle,0x11)==certificate.public_bytes(serialization.Encoding.DER)
        assert attr_value(cert_handle,0x102)==attr_value(pub,0x102)==attr_value(private,0x102)
        sensitive=Attribute(0x123,None,0)
        call('C_GetAttributeValue',[U,U,P,U],s,private,C.byref(sensitive),1,expected=0x11)
        assert sensitive.length==U(-1).value
        short=C.create_string_buffer(1)
        mixed=(Attribute*2)(Attribute(0x11,C.cast(short,P),1),Attribute(0,None,0))
        call('C_GetAttributeValue',[U,U,P,U],s,cert_handle,mixed,2,expected=0x150)
        assert mixed[0].length>1 and mixed[1].length==C.sizeof(U)
        checks.append('DER-identities-private-exponent-protection-and-attribute-buffer-semantics')
        def calls(): return len(counter.read_text().splitlines()) if counter.exists() else 0
        message=buffer(document)
        mechanism=Mechanism(0x40,None,0)
        call('C_SignInit',[U,P,U],s,C.byref(mechanism),private)
        call('C_SignInit',[U,P,U],s,C.byref(mechanism),private,expected=0x90)
        length=U()
        call('C_Sign',[U,P,U,P,P],s,message,len(document),None,C.byref(length))
        assert length.value==256 and calls()==0
        length.value=1
        output=C.create_string_buffer(256)
        call('C_Sign',[U,P,U,P,P],s,message,len(document),output,C.byref(length),expected=0x150)
        assert length.value==256 and calls()==0
        call('C_Sign',[U,P,U,P,P],other,message,len(document),output,C.byref(length),expected=0x91)
        call('C_Sign',[U,P,U,P,P],s,message,len(document),output,C.byref(length))
        assert calls()==1
        assert output.raw==base64.b64decode(data['signatures']['SHA256'])
        call('C_Sign',[U,P,U,P,P],s,message,len(document),output,C.byref(length),expected=0x91)
        checks.append('size-and-short-buffer-do-not-sign;one-effective-call-consumes-operation')
        call('C_VerifyInit',[U,P,U],s,C.byref(mechanism),pub)
        call('C_Verify',[U,P,U,P,U],s,message,len(document),output,256)
        changed=buffer(b'changed')
        call('C_VerifyInit',[U,P,U],s,C.byref(mechanism),pub)
        call('C_Verify',[U,P,U,P,U],s,changed,7,output,256,expected=0xc0)
        checks.append('local-verification-and-tamper-rejection')
        for mechanism_type, name in [(0x40,'SHA256'),(0x41,'SHA384'),(0x42,'SHA512')]:
            mechanism=Mechanism(mechanism_type,None,0)
            call('C_SignInit',[U,P,U],s,C.byref(mechanism),private)
            first=buffer(document[:10]); last=buffer(document[10:])
            call('C_SignUpdate',[U,P,U],s,first,10)
            call('C_SignUpdate',[U,P,U],s,last,len(document)-10)
            before=calls(); length=U()
            call('C_SignFinal',[U,P,P],s,None,C.byref(length))
            assert calls()==before
            length.value=256
            call('C_SignFinal',[U,P,P],s,output,C.byref(length))
            assert calls()==before+1
            assert output.raw==base64.b64decode(data['signatures'][name])
        checks.append('multipart-sha256-sha384-sha512-and-final-length-query')
        prefix=bytes.fromhex('3031300d060960864801650304020105000420')
        encoded=prefix+hashlib.sha256(document).digest()
        mechanism=Mechanism(1,None,0)
        call('C_SignInit',[U,P,U],s,C.byref(mechanism),private)
        length=U(256)
        call('C_Sign',[U,P,U,P,P],s,buffer(encoded),len(encoded),output,C.byref(length))
        assert output.raw==base64.b64decode(data['signatures']['SHA256'])
        before=calls()
        call('C_SignInit',[U,P,U],s,C.byref(mechanism),private)
        call('C_Sign',[U,P,U,P,P],s,buffer(b'raw'),3,output,C.byref(length),expected=0x20)
        assert calls()==before
        checks.append('RSA-PKCS-DigestInfo-not-double-hashed;raw-input-rejected-before-helper')
        # A canceled/invalid response must consume the operation and never retry.
        for script, expected in [
            ('#!/usr/bin/python3\nimport sys\nsys.exit(2)\n',0x50),
            ('#!/usr/bin/python3\nimport json,base64\nprint(json.dumps({"schema_version":1,"signature_base64":base64.b64encode(bytes(256)).decode()}))\n',0xc0),
        ]:
            helper.write_text(script)
            mechanism=Mechanism(0x40,None,0)
            call('C_SignInit',[U,P,U],s,C.byref(mechanism),private)
            call('C_Sign',[U,P,U,P,P],s,message,len(document),output,C.byref(length),expected=expected)
            call('C_Sign',[U,P,U,P,P],s,message,len(document),output,C.byref(length),expected=0x91)
        checks.append('cancellation-and-invalid-helper-signature-terminate-without-retry')
        call('C_SignInit',[U,P,U],s,C.byref(Mechanism(0xd,None,0)),private,expected=0x70)
        call('C_Logout',[U],s)
        assert find(other,3)==[]
        call('C_CloseSession',[U],s)
        call('C_CloseSession',[U],other)
        call('C_Finalize',[P],None)
        checks.append('unsupported-PSS-and-session-cleanup')
        for args in [['--show-info'],['--list-slots'],['--list-mechanisms'],['--list-objects','--login']]:
            tool=subprocess.run(['pkcs11-tool','--module',str(module),*args],capture_output=True,text=True,timeout=30)
            assert tool.returncode==0, f'OpenSC check failed: {args}: {tool.stderr}'
        checks.append('OpenSC-loads-function-table-slots-mechanisms-and-objects')
        helper.write_text(synthetic_helper)
        input_path=temp/'native-input.txt'; input_path.write_bytes(document)
        output_path=temp/'native-signature.bin'
        identifier=hashlib.sha256(certificate.public_bytes(serialization.Encoding.DER)).hexdigest()
        before=calls()
        native=subprocess.run(['pkcs11-tool','--module',str(module),'--slot','8','--login',
            '--sign','--mechanism','SHA256-RSA-PKCS','--id',identifier,
            '--input-file',str(input_path),'--output-file',str(output_path)],
            capture_output=True,text=True,timeout=30)
        assert native.returncode==0, f'OpenSC signing failed: {native.stderr}'
        assert calls()==before+1
        assert output_path.read_bytes()==base64.b64decode(data['signatures']['SHA256'])
        checks.append('OpenSC-effective-signature-with-exact-real-test-arguments')
    result={'schema_version':1,'successful':True,'scope':'native-pkcs11-laboratory',
            'checks':checks,'module_is_elf':module.read_bytes()[:4]==b'\x7fELF',
            'module_sha256':hashlib.sha256(module.read_bytes()).hexdigest(),
            'real_cloud_request_sent':False,'real_credentials_used':False,
            'gnome_authorization_window_tested':False,'browser_or_pjeoffice_tested':False}
    report.parent.mkdir(parents=True,exist_ok=True)
    report.write_text(json.dumps(result,indent=2)+'\n')
    print(f'PKCS11: {len(checks)} laboratory checks passed; report: {report}')

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--module',type=Path,required=True)
    parser.add_argument('--client',required=True)
    parser.add_argument('--report',type=Path,required=True)
    args=parser.parse_args()
    main(args.module,args.client,args.report)
