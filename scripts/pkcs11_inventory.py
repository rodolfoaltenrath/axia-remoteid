"""Read local PKCS11 object counts without names, credentials or cloud calls."""
import argparse
import ctypes as C
import json
from pathlib import Path
from scripts.pkcs11_smoke import Attribute, U, P

def inventory(module):
    library=C.CDLL(str(module))
    def call(name,types,*args):
        fn=getattr(library,name); fn.argtypes=types; fn.restype=U
        rv=fn(*args)
        if rv: raise RuntimeError(f'{name} failed: {rv:#x}')
    call('C_Initialize',[P],None)
    session=U()
    try:
        call('C_OpenSession',[U,U,P,P,P],8,4,None,None,C.byref(session))
        def count(cls):
            value=U(cls); attr=Attribute(0,C.cast(C.byref(value),P),C.sizeof(U))
            call('C_FindObjectsInit',[U,P,U],session,C.byref(attr),1)
            total=0
            while True:
                objects=(U*64)(); found=U()
                call('C_FindObjects',[U,P,U,P],session,objects,64,C.byref(found))
                total+=found.value
                if found.value==0: break
            call('C_FindObjectsFinal',[U],session)
            return total
        certificates=count(1); public=count(2); before=count(3)
        call('C_Login',[U,U,P,U],session,1,None,0)
        private=count(3)
        call('C_Logout',[U],session)
        call('C_CloseSession',[U],session)
        return {'scope':'local PKCS11 inventory only', 'certificate_count':certificates,
            'public_key_count':public,'private_references_before_local_login':before,
            'private_key_reference_count':private,'remote_signature_requested':False,
            'remote_login_registration_or_wallet_requested':False,'credentials_requested':False,
            'certificate_names_or_identifiers_disclosed':False}
    finally:
        call('C_Finalize',[P],None)

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--module',type=Path,required=True)
    parser.add_argument('--report',type=Path)
    args=parser.parse_args()
    result=inventory(args.module)
    if args.report:
        args.report.parent.mkdir(parents=True,exist_ok=True)
        args.report.write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))
