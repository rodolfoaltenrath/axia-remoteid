# DesktopID Linux

Cliente e módulo PKCS#11 experimentais em Rust para usar certificados em nuvem Certisign no Linux, com autorização por senha e código do aplicativo no celular. É uma reimplementação independente do protocolo RemoteID; o código-fonte original C++/Qt não foi recuperado. Este projeto não é um produto oficial da Certisign.

## Estado atual

Em 13/10/2026, no Fedora 44 WSL2, foram confirmados login, cadastro da instalação Linux, leitura de dois certificados e assinatura real RSA-2048/SHA-256. A assinatura passou tanto pelo cliente de terminal quanto pelo módulo PKCS#11/OpenSC, com autorização na janela local e verificação criptográfica do resultado.

Ainda faltam os testes no Fedora físico com GNOME e a integração com Firefox, PJeOffice, Gerid, PJe, eproc, Projudi, e-CAC e assinatura de PDF. Uma assinatura RSA válida não comprova autenticação em todos esses serviços nem produz, por si só, um PDF/PAdES.

Após a reorganização, passaram 38 testes Rust, 19 Python, 14 verificações do executável e 12 PKCS#11/OpenSC, além de fmt e Clippy com avisos tratados como erros. A configuração de CI está pronta; ainda não foi executada no GitHub.

O módulo oferece PKCS#11 2.40, certificados públicos e referências às chaves remotas. Implementa RSA PKCS#1 v1.5 com SHA-256/384/512 e DigestInfo desses algoritmos. SHA-384/512 foram testados em laboratório; a conta real confirmou SHA-256. RSA-PSS, ECDSA e SHA-1 operacional não estão implementados. Cada assinatura efetiva pede autorização própria; não há cache de senha, OTP ou token de sessão.

## Compilar e testar no Fedora

Use Rust/Cargo compatível com o workspace (mínimo declarado 1.88; validação realizada com 1.98.1), Python 3 e OpenSSL de desenvolvimento. Os comandos abaixo usam os pacotes do Fedora:

```bash
sudo dnf install rust cargo gcc openssl-devel clippy rustfmt python3 python3-cryptography opensc zenity
export CARGO_TARGET_DIR="$HOME/.cache/desktopid-rust-target"
python3 -m unittest discover -s tests -v
sh scripts/test_rust_wsl.sh
sh scripts/test_pkcs11_wsl.sh
python3 scripts/check_publication.py
cargo build --release --workspace --locked
python3 scripts/install_pkcs11.py --target "$CARGO_TARGET_DIR/release"
```

Os testes usam dados sintéticos e servidor local; não entram na conta nem solicitam assinaturas reais. Os scripts terminados em `_wsl.sh` também funcionam diretamente no Fedora nativo. A janela de autorização exige uma sessão gráfica com Zenity. O mínimo Rust declarado ainda precisa de validação separada.

O instalador grava a biblioteca em `~/.local/lib/desktopid-linux/libdesktopid_pkcs11.so` e o auxiliar em `~/.local/libexec/desktopid-linux/desktopid-client`. Não registra automaticamente o módulo no navegador ou em p11-kit. A integração de cada aplicação é a próxima etapa.

## Usar a conta e testar uma assinatura

Execute como o usuário Linux que utilizará os programas finais:

```bash
~/.local/libexec/desktopid-linux/desktopid-client connect
~/.local/libexec/desktopid-linux/desktopid-client sign-test --list
python3 -m scripts.sign_pkcs11_test --inventory-only
```

`connect` cria/reutiliza uma identidade local, cadastra uma vez se necessário e consulta a carteira. O cadastro altera a conta. Se já existe uma instalação registrada, preserve sua identidade e os registros de tentativas; não apague arquivos para forçar recadastro depois de um erro ambíguo. `sign-test --list` e `--inventory-only` consultam apenas dados locais.

Para solicitar **uma assinatura real de um texto de laboratório**, pela interface PKCS#11:

```bash
python3 -m scripts.sign_pkcs11_test --certificate-index 1
```

A janela local pede a senha do aplicativo Certisign e o OTP atual. Cancelar não solicita assinatura. Nenhuma repetição automática ocorre após falha. O resultado é verificado contra o documento e o certificado selecionado.

No Windows, com a distribuição WSL chamada `FedoraLinux-44`, use PowerShell na pasta do projeto:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\test_rust_wsl.ps1
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\test_pkcs11_wsl.ps1
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\run_client_wsl.ps1 status
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\sign_pkcs11_wsl.ps1 --inventory-only
```

Para uma nova assinatura real, substitua `--inventory-only` por `--certificate-index 1`. O cadastro inicial pode ser feito com `run_client_wsl.ps1 connect`. Esses comandos interativos devem ser executados pelo usuário, com os segredos digitados localmente.

## Dados locais e distribuição

A identidade fica em `~/.config/desktopid-linux` ou no diretório configurado por XDG_CONFIG_HOME. No WSL já validado ela pertence ao usuário root; no Fedora final use o usuário normal. Preserve a chave, configuração, carteira e registros de tentativas; mantenha diretórios 0700 e arquivos privados 0600. `DESKTOPID_STATE_DIR` permite escolher o estado do módulo; o CLI aceita `--state-dir`. `DESKTOPID_HELPER` permite selecionar um auxiliar absoluto com propriedade e permissões verificadas.

Dados pessoais e credenciais não fazem parte do repositório. O fixture de testes inclui uma **chave privada RSA publicada propositalmente para laboratório**, associada a certificado autoassinado sintético; ela não pertence a uma conta e nunca deve ser usada como identidade real.

Para gerar um pacote pequeno para outra máquina Linux:

```bash
python3 scripts/package_linux.py --target "$CARGO_TARGET_DIR/release"
```

O arquivo em `dist/` inclui somente biblioteca, auxiliar, instalador, instruções, licença e manifesto de hashes. Depois de extrair, execute `python3 install_linux.py --target .` dentro de `desktopid-linux/`. Os binários são vinculados às bibliotecas do sistema de compilação; valide a compatibilidade no destino ou compile no próprio Fedora. A transferência de uma identidade registrada deve ser feita separadamente, de forma privada, corrigindo proprietário e permissões. Não coloque a identidade no pacote ou no GitHub.

Antes de publicar, execute `python3 scripts/check_publication.py` e revise os arquivos selecionados pelo Git. O verificador detecta formatos comuns de dados privados; não substitui essa revisão. Instaladores proprietários, extrações, Ghidra, downloads, compilações e relatórios gerados foram retirados da árvore de fontes ou ignorados. Não são necessários para compilar.

O material antigo desta máquina foi arquivado em `.local-research/`, ignorado pelo Git. A exclusão física foi bloqueada pela revisão automática do ambiente. Não publique essa pasta; ela pode ser apagada manualmente depois de conferir os fontes. Para transportar somente os arquivos publicáveis, execute `python3 -m scripts.package_sources`; o ZIP em `dist/` exclui esse arquivo local e os dados privados.

## Estrutura e continuidade

| Caminho | Função |
| --- | --- |
| `crates/desktopid-client` | Protocolo, estado privado, CLI e auxiliar gráfico |
| `crates/desktopid-pkcs11` | Biblioteca nativa PKCS#11 |
| `tests/reference` | Modelo Python independente, sem acesso à conta |
| `tests` e testes nas crates | Regressões e interoperabilidade |
| `scripts` | Compilação, instalação, pacote e verificações |
| `.github/workflows/ci.yml` | Testes automáticos em contêiner Fedora |

Consulte a [referência do protocolo](docs/PROTOCOL.md) e a [continuidade para desenvolvimento e IA](CONTINUIDADE_IA_FEDORA.md). Código próprio disponibilizado sob [Apache License 2.0](LICENSE); dependências conservam suas próprias licenças.
