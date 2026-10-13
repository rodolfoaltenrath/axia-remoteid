"""Interactive real PKCS11 test. Each invocation requests one laboratory signature."""
import argparse
import base64
import json
import os
from pathlib import Path
import subprocess
import sys
import uuid
from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import padding

def main(certificate_number, inventory_only):
    root=Path(os.environ.get('DESKTOPID_STATE_DIR',str(Path.home()/'.config/desktopid-linux')))
    module=Path.home()/'.local/lib/desktopid-linux/libdesktopid_pkcs11.so'
    helper=Path.home()/'.local/libexec/desktopid-linux/desktopid-client'
    if inventory_only:
        from scripts.pkcs11_inventory import inventory
        print(json.dumps(inventory(module),indent=2))
        return
    if not sys.stdin.isatty(): raise RuntimeError('Execute no terminal interativo')
    if not (os.environ.get('DISPLAY') or os.environ.get('WAYLAND_DISPLAY')):
        raise RuntimeError('Sessão gráfica indisponível; nenhuma assinatura solicitada')
    metadata=root.lstat()
    if root.is_symlink() or not root.is_dir() or metadata.st_uid!=os.geteuid() or metadata.st_mode & 0o077:
        raise RuntimeError('Identidade exige diretório Linux privado do usuário atual')
    if str(root.resolve()).startswith('/mnt/'):
        raise RuntimeError('Guarde identidade e artefatos no sistema de arquivos Linux')
    wallet=json.loads((root/'wallet.json').read_text())
    if not 1<=certificate_number<=len(wallet['certificados']): raise RuntimeError('Número de certificado inexistente')
    item=wallet['certificados'][certificate_number-1]
    certificate=x509.load_der_x509_certificate(base64.b64decode(item['base64']))
    identifier=certificate.fingerprint(hashes.SHA256()).hex()
    parent=root/'pkcs11-tests'
    if not parent.exists(): parent.mkdir(mode=0o700)
    if parent.is_symlink() or parent.stat().st_uid!=os.geteuid() or parent.stat().st_mode & 0o077:
        raise RuntimeError('Diretório de teste exige propriedade/permissões privadas')
    directory=parent/uuid.uuid4().hex
    directory.mkdir(mode=0o700)
    document=b'DesktopID Linux - teste real via PKCS11/OpenSC e autorizacao local.\n'
    for name,payload in [('document.txt',document),('certificate.pem',certificate.public_bytes(serialization.Encoding.PEM)),
                         ('attempt.json',json.dumps({'operation':'pkcs11-laboratory-signature','certificate_number':certificate_number,'result':'requires-review'}).encode())]:
        with (directory/name).open('xb') as output:
            os.chmod(directory/name,0o600); output.write(payload)
    signature=directory/'signature.bin'
    print('Será solicitada UMA assinatura de texto de laboratório pelo módulo PKCS11.',flush=True)
    print('A janela local pedirá senha do aplicativo Certisign e código do celular. Cancelar não solicita assinatura.',flush=True)
    print(f'Artefatos privados: {directory}',flush=True)
    env=os.environ.copy(); env['DESKTOPID_STATE_DIR']=str(root); env['DESKTOPID_HELPER']=str(helper)
    # Output suppressed: OpenSC may display certificate names/identifiers.
    try:
        result=subprocess.run(['pkcs11-tool','--module',str(module),'--slot','8','--login',
            '--sign','--mechanism','SHA256-RSA-PKCS','--id',identifier,
            '--input-file',str(directory/'document.txt'),'--output-file',str(signature)],
            env=env,capture_output=True,timeout=430,umask=0o077)
    except subprocess.TimeoutExpired:
        raise RuntimeError('Tempo do teste excedido; conferir artefatos locais antes de nova assinatura') from None
    if signature.exists(): signature.chmod(0o600)
    if result.returncode!=0:
        safe='Operação cancelada' if b'CKR_FUNCTION_CANCELED' in result.stderr else 'Operação PKCS11 não foi confirmada'
        raise RuntimeError(f'{safe}; artefatos preservados em {directory}. Não houve repetição automática.')
    certificate.public_key().verify(signature.read_bytes(),document,padding.PKCS1v15(),hashes.SHA256())
    with (directory/'result.json').open('x') as output:
        os.chmod(directory/'result.json',0o600)
        json.dump({'scope':'real-pkcs11-laboratory','certificate_number':certificate_number,'signature_verified':True},output)
    print('Assinatura real via PKCS11/OpenSC aceita e verificada no Linux.')
    print(f'Artefatos privados salvos em {directory}')

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--certificate-index',type=int,default=1)
    parser.add_argument('--inventory-only',action='store_true')
    args=parser.parse_args()
    try: main(args.certificate_index,args.inventory_only)
    except Exception as error:
        print(f'Erro: {error}',file=sys.stderr); sys.exit(1)
