# Axia RemotedID — instalação e desenvolvimento

Guia técnico para preparar a versão alpha, instalar a integração com o Firefox e contribuir com o projeto. Para uma apresentação do produto, consulte o [README](../README.md). Os resultados de validação e o histórico técnico estão na [continuidade do projeto](../CONTINUIDADE_IA_FEDORA.md).

Para instalar o pacote pré-compilado sem ferramentas de desenvolvimento, siga o [guia de testes da alpha](TESTING.md). O pacote inclui `install.sh`, instruções para usuários e um arquivo SHA-256 separado. A build de distribuição atual destina-se ao Fedora 44 x86_64.

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

### Executável local para teste no Fedora

Com as dependências de compilação instaladas, execute na pasta do projeto:

```bash
sh scripts/build_fedora.sh
./dist/fedora/axia-remotedid
```

O script compila em modo release e copia o executável `axia-remotedid`, o auxiliar compatível `desktopid-client` e a biblioteca `libdesktopid_pkcs11.so` para `dist/fedora/`. As pastas `/target/` e `/dist/` já estão no `.gitignore`, incluindo todos esses arquivos. A compilação não acessa a conta.

Na primeira abertura gráfica, o Axia RemotedID oferece **Ativar** ou **Agora não** para iniciar automaticamente ao entrar na sessão do Fedora. A escolha é lembrada em `~/.config/axia-remotedid/preferences.json`, separada da identidade. Se ativado, mostra o estado local dos certificados ao entrar na sessão; conectar ou atualizar continua sendo uma ação do usuário, sem login automático. Para mudar a escolha depois, use `~/.local/bin/axia-remotedid autostart --enable` ou `--disable`; sem essas opções, `autostart` mostra o estado atual. A entrada XDG fica em `~/.config/autostart/axia-remotedid.desktop`. Esses caminhos seguem `XDG_CONFIG_HOME` quando definido.

Ao iniciar sem argumentos (ou com o comando `gui`), o cliente abre uma janela Zenity com e-mail, senha e nome opcional do computador. O botão **Conectar e cadastrar** faz login, cadastra a instalação Linux uma vez e consulta os certificados. Uma instalação já cadastrada oferece **Atualizar certificados**, reutilizando a identidade existente. Cancelar o formulário não cria uma identidade nem acessa a conta. O resultado e os erros aparecem em janelas; uma falha de cadastro não provoca repetição automática.

Os comandos de terminal continuam disponíveis em `./dist/fedora/axia-remotedid --help`. `login-check` testa somente o login, sem cadastrar; `connect` faz a conexão e o cadastro pelo terminal. A interface gráfica exige uma sessão gráfica e o pacote `zenity`.

Para validar os arquivos gerados com dados sintéticos:

```bash
python3 scripts/rust_cli_smoke.py --binary dist/fedora/desktopid-client --report test-results/fedora-cli.json
python3 scripts/pkcs11_smoke.py --module "$PWD/dist/fedora/libdesktopid_pkcs11.so" --client "$PWD/dist/fedora/desktopid-client" --report test-results/fedora-pkcs11.json
```

Ao usar a biblioteca diretamente dessa pasta em uma aplicação, configure `DESKTOPID_HELPER` com o caminho absoluto de `dist/fedora/desktopid-client`. Para instalar ambos nos caminhos padrão do usuário, use `python3 scripts/install_pkcs11.py --target dist/fedora`.

### Instalar a alpha e integrar o Firefox nativo

Depois de conectar a conta e salvar a carteira pela GUI, feche o Firefox e execute:

```bash
python3 scripts/install_alpha.py --target dist/fedora
```

O instalador copia os binários para os caminhos do usuário, cria **Axia RemotedID Alpha** no menu do GNOME e registra o módulo **DesktopID Linux Alpha** no perfil padrão do Firefox nativo. Usa a biblioteca NSS já instalada com o navegador, sem exigir `sudo` ou `nss-tools`. Se houver vários perfis de instalação, informe `--profile /caminho/absoluto/do/perfil`. Um perfil em uso é recusado; o instalador mantém a trava do Firefox durante a atualização. Antes de adicionar o módulo, salva cópias privadas das bases NSS em `~/.local/state/desktopid-linux/browser-backups/`.

Ao reabrir o Firefox e escolher acesso por certificado em um site, o navegador consulta o módulo. A autorização de uma assinatura usa a janela local com senha e OTP. A GUI de conexão pode estar fechada; não é necessário manter um processo Axia RemotedID aberto. Esse instalador integra somente o Firefox nativo; outros navegadores e versões Flatpak exigem integração própria. Não altera a confiança em autoridades certificadoras nem as versões de TLS do navegador.

O módulo não exige um login preliminar para consultar referências às chaves remotas, evitando a janela genérica de autenticação protegida do Firefox. A chave real permanece remota e não extraível. A primeira assinatura pede senha e OTP na janela Axia RemotedID e autoriza pedidos seguintes do mesmo aplicativo com esse certificado por até dez minutos. Fechar e reabrir abas conserva essa autorização enquanto o processo do Firefox continuar aberto. Fechar o navegador, descarregar o módulo ou efetuar logout do dispositivo encerra o cache local. O prazo não se renova com o uso; outro certificado exige outra autorização. Ao atualizar a biblioteca, reinicie o navegador para carregar a nova versão.

Uma falha de assinatura, de verificação ou de sessão descarta a autorização e encerra o auxiliar. A operação não é repetida automaticamente; um novo pedido solicitará nova senha e OTP. Reutilização, expiração, troca de identidade/certificado, cancelamento e encerramento passaram em testes locais com dados sintéticos. O servidor pode rejeitar o token antes do prazo local; a nova autorização ainda precisa de teste real no GERID.

O comando instalado fica em `~/.local/bin/axia-remotedid`. Os caminhos internos `desktopid-linux`, o nome da biblioteca e o apelido NSS `DesktopID Linux Alpha` são preservados para reutilizar o cadastro e a integração já existentes. As telas, o nome do token e o pacote usam Axia RemotedID.

Para instalar apenas os binários e o atalho, use `--skip-firefox`. Em uma instalação nova, use essa opção primeiro, conecte a conta pelo menu e depois execute o instalador completo com o Firefox fechado.

Validação sintética da integração NSS, sem acessar conta ou perfil real:

```bash
python3 scripts/firefox_nss_smoke.py --module dist/fedora/libdesktopid_pkcs11.so --client dist/fedora/desktopid-client
```

O teste confirma carregamento pelo NSS, enumeração de certificado de laboratório, persistência, reinstalação sem duplicação, backup privado e preservação de um módulo conflitante. O usuário confirmou acesso real ao GERID/INSS pelo Firefox no Fedora nativo; outros serviços continuam pendentes.

## Gerar o RPM de distribuição

Compile a versão release e gere o pacote no Fedora 44 x86_64:

```bash
sudo dnf install rpm-build desktop-file-utils openssl-libs
sh scripts/build_fedora.sh
python3 scripts/package_rpm.py --target dist/fedora
```

Também é possível empacotar com Podman, sem instalar as ferramentas RPM no host:

```bash
python3 scripts/package_rpm.py --target dist/fedora --container
```

O RPM e seu checksum ficam em `dist/`, ignorados pelo Git. O pacote declara as dependências de execução, instala o comando em `/usr/bin/axia-remotedid`, o auxiliar em `/usr/libexec/axia-remotedid` e o módulo em `/usr/lib64/axia-remotedid`. A validação dos caminhos do sistema exige propriedade root, permissões protegidas e ausência de links simbólicos nos componentes.

A integração NSS ocorre na sessão do usuário, pela interface gráfica, com o Firefox fechado. Nenhum script de instalação ou remoção do RPM modifica perfis ou estados de contas. A migração do módulo legado para o módulo do RPM exige ação do usuário, mantém backup e tenta restaurar o módulo anterior se a troca falhar. O atalho do sistema usa um ID próprio; a primeira abertura oculta somente os atalhos legados reconhecidos do projeto e atualiza seu próprio alias, preservando arquivos de terceiros e os dados da identidade.

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

O arquivo em `dist/` inclui somente biblioteca, auxiliar, instaladores, integração Firefox, instruções, licença e manifesto de hashes. Depois de extrair, execute `python3 install_alpha.py --target .` dentro de `axia-remotedid/`, com a conta já conectada e o Firefox fechado. Em uma instalação nova, use primeiro `--skip-firefox`, conecte a conta pela GUI e depois execute o instalador completo. `python3 install_linux.py --target .` continua disponível para instalar somente biblioteca e auxiliar. Os binários são vinculados às bibliotecas do sistema de compilação; valide a compatibilidade no destino ou compile no próprio Fedora. A transferência de uma identidade registrada deve ser feita separadamente, de forma privada, corrigindo proprietário e permissões. Não coloque a identidade no pacote ou no GitHub.

Antes de publicar, execute `python3 scripts/check_publication.py` e revise os arquivos selecionados pelo Git. O verificador detecta formatos comuns de dados privados; não substitui essa revisão. Instaladores proprietários, extrações, Ghidra, downloads, compilações e relatórios gerados foram retirados da árvore de fontes ou ignorados. Não são necessários para compilar.

## Estrutura e continuidade

| Caminho | Função |
| --- | --- |
| `crates/desktopid-client` | Protocolo, estado privado, CLI e auxiliar gráfico |
| `crates/desktopid-pkcs11` | Biblioteca nativa PKCS#11 |
| `tests/reference` | Modelo Python independente, sem acesso à conta |
| `tests` e testes nas crates | Regressões e interoperabilidade |
| `scripts` | Compilação, instalação, pacote e verificações |
| `.github/workflows/ci.yml` | Testes automáticos em contêiner Fedora |

Consulte a [referência do protocolo](PROTOCOL.md) e a [continuidade para desenvolvimento e IA](../CONTINUIDADE_IA_FEDORA.md). Código próprio disponibilizado sob [Apache License 2.0](../LICENSE); dependências conservam suas próprias licenças.
