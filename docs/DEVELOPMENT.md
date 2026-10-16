# Axia RemotedID — instalação e desenvolvimento

Guia técnico para preparar a versão alpha, instalar a integração com o Firefox e contribuir com o projeto. Para uma apresentação do produto, consulte o [README](../README.md). Os resultados de validação e o histórico técnico estão na [continuidade do projeto](../CONTINUIDADE_IA_FEDORA.md).

Para instalar o pacote pré-compilado sem ferramentas de desenvolvimento, siga o [guia de testes da alpha](TESTING.md). A alpha.3 contém executáveis nativos, instruções e checksum, sem scripts de execução; as versões anteriores permanecem no histórico de releases. A build de distribuição atual destina-se ao Fedora 44 x86_64.

## Compilar e testar no Fedora

Use Rust/Cargo compatível com o workspace (mínimo declarado 1.88; validação realizada com 1.98.1), GTK 4 e OpenSSL de desenvolvimento. Aplicativo, empacotamento, diagnóstico e testes são Rust. As antigas regressões Python foram incorporadas aos testes das crates e às ferramentas de integração. Os comandos abaixo usam os pacotes do Fedora:

```bash
sudo dnf install rust cargo gcc openssl-devel gtk4-devel clippy rustfmt dbus-daemon opensc nss desktop-file-utils
export CARGO_TARGET_DIR="$HOME/.cache/desktopid-rust-target"
sh scripts/test_rust_wsl.sh
sh scripts/test_pkcs11_wsl.sh
cargo xtask check-publication
cargo build --release --workspace --locked
"$CARGO_TARGET_DIR/release/desktopid-client" install-user --target "$CARGO_TARGET_DIR/release"
```

Os testes usam dados sintéticos e servidor local; não entram na conta nem solicitam assinaturas reais. Os scripts terminados em `_wsl.sh` também funcionam diretamente no Fedora nativo. A janela de autorização é GTK nativa e exige uma sessão gráfica. Nenhum interpretador é usado pelo aplicativo instalado. O mínimo Rust declarado ainda precisa de validação separada.

A CI executa a suíte Cargo e as integrações compiladas no job Fedora. Não há job Python, `requirements-test.txt` ou código Python de testes no projeto. As bibliotecas GTK/GStreamer do Fedora ainda podem instalar Python indiretamente para ferramentas do sistema; isso não torna o interpretador uma rotina do Axia nem autoriza remover o Python do sistema.

### Cobertura migrada dos 26 testes Python

| Suíte anterior | Cobertura Rust vigente |
| --- | --- |
| Protocolo: 14 testes | `crates/desktopid-client/tests/protocol.rs`: canonicalização com bytes literais e limites int64; PIN/OTP e payload exato; digest/tamanho; respostas inválidas e ID opcional; ordem/split do keyName; prazo original; assinatura RSA e rejeição de alteração |
| Firefox: 6 testes | `axia-desktop::browsers::tests`: prioridade Install, múltiplos perfis sem escolha implícita, fcntl mantido por outro processo e recusa de symlink; `axia-desktop::desktop::tests`: atalho com espaços/metacaracteres, validação XDG e recusa de atalho estrangeiro |
| Desktop/RPM: 6 testes | Testes de migração dos próprios atalhos/alias, cancelamento sem registro e ausência de conta sem configuração; `cargo xtask cli-smoke` para CLI sem GUI/estado; `native-ui-smoke` para recusa de conta root; `rpm-runtime-smoke` para migração NSS real com backup e preservação após remoção |

O fixture histórico `python-vectors.json` permanece como dados públicos imutáveis gerados pela implementação independente anterior; não executa Python. A nova regressão RSA calcula `s^e mod n` e confere os bytes EMSA-PKCS1-v1_5/SHA-256, sem depender do verificador de produção. O bloqueio Firefox é exercitado por um processo separado com `fcntl`, em vez de simular um bloqueio no mesmo processo. Cancelamento substitui apenas a fronteira gráfica e verifica que o fluxo de configuração não chama o registro nem altera o perfil. Os testes NSS e RPM usam as bibliotecas/CLI reais em estado sintético.

O [mapa detalhado da migração](TEST_MIGRATION.md) relaciona os 26 cenários anteriores aos testes atuais. Seleções vazias são tratadas como cancelamento; índices inválidos são recusados antes de iniciar o registro NSS. Os testes verificam a preservação do estado local e dos perfis nesses casos.

O instalador grava a biblioteca em `~/.local/lib/desktopid-linux/libdesktopid_pkcs11.so` e o auxiliar em `~/.local/libexec/desktopid-linux/desktopid-client`. Não registra automaticamente o módulo no navegador ou em p11-kit. A configuração dos navegadores é uma ação posterior do usuário, pelo painel nativo.

### Executável local para teste no Fedora

Com as dependências de compilação instaladas, execute na pasta do projeto:

```bash
sh scripts/build_fedora.sh
./dist/fedora/axia-remotedid
```

O script compila em modo release e copia o executável `axia-remotedid`, o auxiliar compatível `desktopid-client` e a biblioteca `libdesktopid_pkcs11.so` para `dist/fedora/`. As pastas `/target/` e `/dist/` já estão no `.gitignore`, incluindo todos esses arquivos. A compilação não acessa a conta.

Na primeira abertura gráfica, o Axia RemotedID oferece **Ativar** ou **Agora não** para iniciar automaticamente ao entrar na sessão do Fedora. A escolha é lembrada em `~/.config/axia-remotedid/preferences.json`, separada da identidade. Se ativado, inicia somente na bandeja, sem janela ou item na dock. O menu oferece **Abrir Axia RemotedID**, a quantidade local de certificados, **Iniciar com o sistema** e **Sair**. Fechar a interface deixa a bandeja ativa; sair da bandeja não desativa a inicialização no próximo login. Conectar ou atualizar continua sendo uma ação do usuário, sem login automático.

O residente usa StatusNotifierItem/D-Bus diretamente em Rust (`ksni`), sem janela GTK oculta, daemon de conta ou novo cache de credenciais. Uma trava `flock` privada evita residentes duplicados e é liberada pelo sistema ao encerrar o processo. O residente aguarda silenciosamente o suporte à bandeja e se registra novamente após reiniciar esse suporte. No GNOME, o RPM recomenda **AppIndicator and KStatusNotifierItem Support** somente quando o GNOME já está instalado. Ao abrir a GUI com autostart habilitado, o aplicativo tenta ativar a extensão instalada por D-Bus se o host da bandeja ainda não existe; não executa scripts nem altera essa preferência a cada login. Sem suporte a indicadores, a inicialização continua sem janelas e o aplicativo permanece acessível pelo menu do sistema. Isso está na build de desenvolvimento; o RPM alpha.1 publicado mantém o diálogo anterior.

Para mudar a escolha pelo terminal, use `~/.local/bin/axia-remotedid autostart --enable` ou `--disable`; sem essas opções, `autostart` mostra o estado atual. A entrada XDG fica em `~/.config/autostart/axia-remotedid.desktop`. Esses caminhos seguem `XDG_CONFIG_HOME` quando definido. Para testar o residente e o menu, execute `cargo xtask tray-smoke --binary target/debug/desktopid-client` (requer `dbus-daemon`). A ferramenta cria um barramento privado, um host simulado e um auxiliar GUI compilado, sem conta real. Use um ambiente sem o auxiliar RPM instalado, para que o residente selecione o auxiliar sintético no home temporário.

Ao iniciar sem argumentos ou com `gui`, o cliente abre um painel GTK com estado local, **Conectar conta**/**Atualizar certificados** e **Configurar navegadores**. Login e autorização usam campos nativos com buffer de senha protegido e opção de exibir o texto. As operações lentas executam em workers, mantendo a interface responsiva. Cancelar o formulário não cria identidade nem acessa a conta; uma falha de cadastro não provoca repetição automática. O comando `pkcs11-broker` conserva o protocolo e a sessão somente em memória.

O painel relê o cadastro salvo antes de escolher a ação de conexão e após sucesso, erro ou cancelamento. Se o cadastro já foi confirmado e a consulta da carteira falha, passa a oferecer **Atualizar certificados** na mesma janela, sem novo login ou cadastro. **Configurar navegadores** depende de uma carteira local validada; uma carteira válida anterior permanece disponível se uma atualização falhar. Tentativas de cadastro com resultado desconhecido continuam bloqueadas, e erro na leitura da identidade não autoriza cadastrar outra vez. Detalhes e testes em [correção 3: recuperação da conexão](FIX_03_CONNECTION.md).

Os comandos de terminal continuam disponíveis em `./dist/fedora/axia-remotedid --help`. `login-check` testa apenas login; `connect` cadastra uma vez. `setup-browsers --list` detecta os navegadores locais sem alterar suas bases. Nenhum navegador é instalado pelo RPM.

Para validar os arquivos gerados com dados sintéticos:

```bash
cargo xtask cli-smoke --binary dist/fedora/desktopid-client --report test-results/fedora-cli.json
cargo xtask pkcs11-smoke --module "$PWD/dist/fedora/libdesktopid_pkcs11.so" --client "$PWD/dist/fedora/desktopid-client" --report test-results/fedora-pkcs11.json
```

Ao usar a biblioteca diretamente dessa pasta em uma aplicação, configure `DESKTOPID_HELPER` com o caminho absoluto de `dist/fedora/desktopid-client`. Para instalar ambos nos caminhos padrão do usuário, use `./dist/fedora/axia-remotedid install-user --target dist/fedora`.

### Instalar e configurar os navegadores existentes

```bash
./dist/fedora/axia-remotedid install-user --target dist/fedora
~/.local/bin/axia-remotedid
```

O instalador compilado copia os dois ELF e cria o atalho no usuário, preservando identidade, carteira, journals e preferências. Pelo painel, conecte a conta e clique em **Configurar navegadores**. Feche os navegadores selecionados antes de confirmar. Firefox apresenta todos os perfis existentes de `profiles.ini`, incluindo secundários, identificando padrões de instalação e geral. A localização exibida no diálogo distingue nomes repetidos. Chromium, Google Chrome, Brave, Edge, Vivaldi e Opera nativos são detectados por seus executáveis. A base Chromium compartilhada é configurada uma vez, sem instalar qualquer navegador.

A descoberta também inclui LibreWolf nativo, com seus próprios perfis e a precedência da raiz legada documentada pelo produto. Busca comandos em PATH e nos diretórios conhecidos do sistema/usuário, além de lançadores desktop reconhecidos que podem apontar para locais como `/opt`. Não executa os comandos desses lançadores. Cada alvo descoberto inclui `installations` no JSON, com identificador do produto, nome e executáveis canônicos. Os rótulos de seleção distinguem Firefox de LibreWolf; `setup-firefox` sem `--profile` conserva a escolha restrita ao Firefox. Catálogo, formatos aceitos e limites em [correção 5: descoberta de navegadores](FIX_05_BROWSER_DISCOVERY.md). Zen, Floorp, Snap e AppImage não foram habilitados nesta entrega.

O painel mantém os resultados por navegador/perfil durante a sessão da janela, separados do cadastro/carteira. Distingue sucesso, necessidade de ação e falha, com causas NSS e estado de recuperação; uma falha não descarta sucessos de outros alvos. Cancelar conserva os resultados anteriores. A instrução de reabrir navegadores exige pelo menos um alvo configurado na tentativa. **Contrato JSON alterado na alpha.2:** `setup-browsers --yes` retorna um objeto com `schema_version: 1`, `status`, `results` e `notices`, em vez do array anterior. Código 0 exige sucesso total; parcial, falha e nenhum alvo retornam 1 com o relatório em stdout. O inventário `--list` e o objeto de `setup-firefox --yes` permanecem compatíveis. Detalhes em [correção 6: resultados por alvo](FIX_06_BROWSER_RESULTS.md).

**Correção incluída na alpha.3:** `browsers/selection.rs` separa a enumeração dos perfis da elegibilidade para configuração. O inventário `--list` continua incluindo todos os perfis existentes. A seleção gráfica e o lote `--yes` omitem perfis Firefox/LibreWolf que ainda não possuem os três arquivos SQL NSS exigidos pelo registro, com orientação em `notices` para abrir e fechar aquele perfil no navegador. Um perfil com apenas `times.json` não produz mais um falso resultado parcial junto do perfil principal. Perfis secundários inicializados continuam disponíveis, e a criação da base Chromium permanece inalterada. Links, tipos de arquivo inadequados e erros de inspeção não são reinterpretados como perfil vazio: o registro conserva as validações anteriores. A checagem é somente de metadados; não abre NSS, cria perfil ou certifica integridade dos arquivos. Uma solicitação explícita por perfil não inicializado recebe `needs_action` com motivo adicional `profile_not_initialized`, sem trava ou backup. Os campos e a versão do JSON permanecem; consumidores devem aceitar esse novo motivo. A seleção implícita de `setup-firefox` conserva as regras de prioridade e ambiguidade.

A seleção da base Chromium respeita `~/.pki/nssdb` quando existente e, nas versões recentes, `${XDG_DATA_HOME:-~/.local/share}/pki/nssdb`. Quando ainda não existe base, cria uma base privada no caminho legado que também é reconhecido pelas versões recentes. A decisão segue a [documentação do Chromium](https://chromium.googlesource.com/chromium/src/+/main/docs/linux/cert_management.md). Flatpak é identificado como uma integração ainda não validada e não é configurado por essa rotina.

A integração usa NSS diretamente em Rust, sem `modutil` ou scripts. Mantém o `fcntl` do Firefox; Chromium exige processos do navegador fechados e uma trava própria entre instâncias Axia. A detecção compartilha os aliases da descoberta e verifica também o executável real do processo. Há novas verificações antes de abrir NSS e antes de modificar o módulo. Faz backup privado das bases antes de uma alteração e restaura os arquivos anteriores se o registro falha, após fechar NSS. Se detectar Chromium aberto ou não puder verificar os processos durante a recuperação, suspende a restauração e conserva o backup, sem sobrescrever a base em uso. A trava própria não impede que o Chromium seja aberto durante a configuração: mantenha os navegadores fechados até a conclusão. Detalhes em [correção 2: Chromium em execução](FIX_02_CHROMIUM.md). Um módulo de outro aplicativo com o mesmo apelido é preservado. Os caminhos do módulo precisam ser instalações protegidas do usuário ou o caminho root conhecido do RPM. Não modifica confiança em CAs nem a política de TLS.

Para uma configuração explícita e local pelo terminal, `setup-browsers --yes` configura todos os alvos detectados, incluindo perfis secundários; `--family firefox` ou `--family chromium` restringe a família. A compatibilidade `setup-firefox --yes --profile /perfil --module /biblioteca` permanece para diagnóstico e testes sintéticos. Sem `--profile`, esse comando aceita um único padrão de instalação, ou um único padrão geral quando não há padrões de instalação, ou um único perfil sem padrão. Casos ambíguos exigem escolha explícita. Sem `--yes`, a configuração exige interação gráfica. O inventário JSON ganha o campo opcional `firefox_default`, com os valores `installation` ou `profile`; os campos anteriores são mantidos. Detalhes e regressões em [correção 4: perfis secundários](FIX_04_FIREFOX_PROFILES.md).

O módulo não exige um login preliminar para consultar referências às chaves remotas, evitando a janela genérica de autenticação protegida do Firefox. A chave real permanece remota e não extraível. A primeira assinatura pede senha e OTP na janela Axia RemotedID e autoriza pedidos seguintes do mesmo aplicativo com esse certificado por até dez minutos. Fechar e reabrir abas conserva essa autorização enquanto o processo do Firefox continuar aberto. Fechar o navegador, descarregar o módulo ou efetuar logout do dispositivo encerra o cache local. O prazo não se renova com o uso; outro certificado exige outra autorização. Ao atualizar a biblioteca, reinicie o navegador para carregar a nova versão.

Uma falha de assinatura, de verificação ou de sessão descarta a autorização e encerra o auxiliar. A operação não é repetida automaticamente; um novo pedido solicitará nova senha e OTP. Reutilização, expiração, troca de identidade/certificado, cancelamento e encerramento passaram em testes locais com dados sintéticos. O servidor pode rejeitar o token antes do prazo local; a nova autorização ainda precisa de teste real no GERID.

O comando instalado fica em `~/.local/bin/axia-remotedid`. Os caminhos internos `desktopid-linux`, o nome da biblioteca e o apelido NSS `DesktopID Linux Alpha` são preservados para reutilizar o cadastro e a integração já existentes. As telas, o nome do token e o pacote usam Axia RemotedID.

O comando `install-user` instala somente os binários e o atalho. Conectar a conta e configurar os navegadores continuam sendo ações explícitas no painel.

Validação sintética da integração NSS, sem acessar conta ou perfil real:

```bash
cargo xtask firefox-nss-smoke --module dist/fedora/libdesktopid_pkcs11.so --client dist/fedora/desktopid-client
```

O teste confirma carregamento pelo NSS, enumeração de certificado de laboratório, persistência, reinstalação sem duplicação, backup privado e preservação de um módulo conflitante. O usuário confirmou acesso real ao GERID/INSS pelo Firefox no Fedora nativo; outros serviços continuam pendentes.

## Gerar o RPM de distribuição

Compile a versão release e gere o pacote no Fedora 44 x86_64:

```bash
sudo dnf install rpm-build desktop-file-utils openssl-libs
sh scripts/build_fedora.sh
cargo xtask package-rpm --target dist/fedora
```

Também é possível empacotar com Podman, sem instalar as ferramentas RPM no host:

```bash
cargo xtask package-rpm --target dist/fedora --container
```

O RPM e seu checksum ficam em `dist/`, ignorados pelo Git. O pacote declara as dependências de execução, instala o comando em `/usr/bin/axia-remotedid`, o auxiliar em `/usr/libexec/axia-remotedid` e o módulo em `/usr/lib64/axia-remotedid`. A validação dos caminhos do sistema exige propriedade root, permissões protegidas e ausência de links simbólicos nos componentes.

A integração NSS ocorre na sessão do usuário, pela interface gráfica, com o Firefox fechado. Nenhum script de instalação ou remoção do RPM modifica perfis ou estados de contas. A migração do módulo legado para o módulo do RPM exige ação do usuário, mantém backup e tenta restaurar o módulo anterior se a troca falhar. O atalho do sistema usa um ID próprio; a primeira abertura oculta somente os atalhos legados reconhecidos do projeto e atualiza seu próprio alias, preservando arquivos de terceiros e os dados da identidade.

A seleção automática do RPM exige módulo, auxiliar e lançador protegidos, com identificador de runtime nativo compatível; auxiliar e lançador também precisam ser hardlinks do mesmo ELF. Um RPM antigo ou incompleto não oculta a instalação por usuário nem prevalece sobre ela. O instalador e o empacotador recusam componentes antigos ou misturados antes de substituir os arquivos existentes. Builds anteriores sem identificador exigem recompilar e reinstalar o par módulo/auxiliar. A configuração dos navegadores pode trocar o registro exato do RPM obsoleto pelo módulo nativo do usuário, mediante a ação explícita no botão, com backup, rollback e preservação de módulos de terceiros. Detalhes em [correção 1: seleção de runtime](FIX_01_RUNTIME.md).

## Usar a conta e testar uma assinatura

Execute como o usuário Linux que utilizará os programas finais:

```bash
~/.local/libexec/desktopid-linux/desktopid-client connect
~/.local/libexec/desktopid-linux/desktopid-client sign-test --list
cargo xtask sign-pkcs11-test --inventory-only
```

`connect` cria/reutiliza uma identidade local, cadastra uma vez se necessário e consulta a carteira. O cadastro altera a conta. Se já existe uma instalação registrada, preserve sua identidade e os registros de tentativas; não apague arquivos para forçar recadastro depois de um erro ambíguo. `sign-test --list` e `--inventory-only` consultam apenas dados locais.

Para solicitar **uma assinatura real de um texto de laboratório**, pela interface PKCS#11:

```bash
cargo xtask sign-pkcs11-test --certificate-index 1
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
cargo xtask package-linux --target "$CARGO_TARGET_DIR/release"
```

O arquivo em `dist/` inclui somente executáveis ELF, módulo, instruções, licença e manifesto de hashes. Depois de extrair a alpha.3, execute `./axia-remotedid install-user --target .` ou abra o painel e escolha **Instalar para este usuário**. Não inclui Python ou instaladores shell. Os binários são vinculados às bibliotecas do sistema de compilação; valide a compatibilidade no destino ou compile no próprio Fedora. A transferência de uma identidade registrada deve ser feita separadamente, de forma privada, corrigindo proprietário e permissões. Não coloque a identidade no pacote ou no GitHub.

Antes de publicar, execute `cargo xtask check-publication` e revise os arquivos selecionados pelo Git. O verificador detecta formatos comuns de dados privados; não substitui essa revisão. Instaladores proprietários, extrações, Ghidra, downloads, compilações e relatórios gerados foram retirados da árvore de fontes ou ignorados. Não são necessários para compilar.

### Ferramentas compiladas de desenvolvimento

Execute os comandos na raiz do repositório. `cargo xtask` compila e executa a ferramenta `axia-devtools`, separada do aplicativo. Ela não entra no RPM nem no pacote Linux. Para usá-la diretamente, compile com `cargo build --release --locked -p axia-devtools` e execute `target/release/axia-devtools` a partir da raiz do projeto. `--root` permite selecionar outra árvore de fontes; os caminhos de saída e de artefatos continuam relativos ao diretório de trabalho.

```bash
cargo xtask check-publication
cargo xtask package-sources --output dist/desktopid-linux-source.zip
cargo xtask package-linux --target dist/fedora
cargo xtask package-rpm --target dist/fedora --container
cargo xtask fedora-diagnostic --module dist/fedora/libdesktopid_pkcs11.so
cargo xtask generate-vectors --output dist/laboratory-vectors.json
```

Esses comandos substituem `check_publication.py`, `package_sources.py`, `package_linux.py`, `package_rpm.py`, `fedora_diagnostic.py` e `generate_rust_vectors.py`. O empacotamento RPM ainda usa `rpmbuild` no host ou Podman com as ferramentas de construção do Fedora; são dependências do desenvolvedor, não executáveis adicionados ao aplicativo. ZIP e tar.gz são escritos por bibliotecas Rust, com metadados fixos, e recebem checksum SHA-256. Os pacotes de execução incluem uma lista explícita de arquivos, sem estados de contas ou testes Python. O ZIP de fontes conserva os testes e usa os candidatos selecionados pelo Git, verificando os mesmos bytes que serão arquivados.

O gerador exige uma saída nova e não altera o fixture histórico `python-vectors.json`, sua exceção de publicação ou sua chave pública de laboratório. Novos dados usam SHA-256/384/512; SHA-1 permanece somente no fixture histórico, respeitando a política criptográfica do Fedora. O teste compara os valores concatenados com o fixture anterior e verifica as assinaturas offline. Os testes do verificador de publicação também passaram para Rust, incluindo arquivos privados rastreados que o `.gitignore` não consegue excluir.

O verificador também recusa fontes Python, bytecode e manifestos de dependências Python. A exceção restrita ao fixture histórico permanece com o mesmo hash, aceitando seu conteúdo em LF ou CRLF; não libera cópias ou alterações desse fixture.

Os nove scripts restantes de inventário, assinatura e testes de integração também foram substituídos. Não há arquivos Python em `scripts/`. O auxiliar sintético de assinatura é uma cópia do ELF de desenvolvimento, ativada somente no home privado de laboratório; ele usa a chave privada sintética publicada deliberadamente no fixture, nunca credenciais ou chaves de conta. Os testes rodam em processos com HOME/XDG/estado próprios, sem alterar o ambiente do processo principal.

```bash
cargo xtask cli-smoke --binary dist/fedora/desktopid-client --report test-results/cli.json
cargo xtask native-ui-smoke --target "$CARGO_TARGET_DIR"
cargo xtask tray-smoke --binary dist/fedora/desktopid-client
cargo xtask pkcs11-smoke --module dist/fedora/libdesktopid_pkcs11.so --client dist/fedora/desktopid-client --report test-results/pkcs11.json
cargo xtask firefox-nss-smoke --module dist/fedora/libdesktopid_pkcs11.so --client dist/fedora/desktopid-client
```

`pkcs11-smoke` valida as 14 verificações de ABI/OpenSC, inventário sem ler a chave privada da instalação e o comando interativo em PTY privado: assinatura sintética válida, cancelamento e resposta inválida, sem repetição automática. `sign-pkcs11-test` continua exigindo terminal e sessão gráfica para uma assinatura real; `--inventory-only` é somente local. Seleção de módulo/auxiliar prefere a instalação protegida do RPM quando presente e aceita caminhos explícitos para laboratório.

## Estrutura e continuidade

| Caminho | Função |
| --- | --- |
| `crates/desktopid-client` | Protocolo, estado privado e workflows compartilhados, sem dependência gráfica |
| `crates/axia-desktop` | CLI, painel GTK, autorização, instalação, detecção e configuração de navegadores e bandeja |
| `crates/desktopid-pkcs11` | Biblioteca nativa PKCS#11 |
| `crates/axia-devtools` | Empacotamento, publicação, diagnóstico, fixtures, inventário e testes de integração em Rust (`cargo xtask`) |
| `crates/*/tests` e módulos `#[cfg(test)]` | Regressões Rust, vetores históricos e interoperabilidade |
| `scripts` | Entradas shell/PowerShell para compilação e ferramentas Rust, inclusive no WSL |
| `.github/workflows/ci.yml` | Testes automáticos em contêiner Fedora |

Consulte a [referência do protocolo](PROTOCOL.md) e a [continuidade para desenvolvimento e IA](../CONTINUIDADE_IA_FEDORA.md). Código próprio disponibilizado sob [Apache License 2.0](../LICENSE); dependências conservam suas próprias licenças.

## Validar a interface nativa e o isolamento do módulo

```bash
cargo xtask native-ui-smoke --target "$CARGO_TARGET_DIR"
cargo xtask firefox-nss-smoke --module "$CARGO_TARGET_DIR/debug/libdesktopid_pkcs11.so" --client "$CARGO_TARGET_DIR/debug/desktopid-client"
```

O teste GTK usa um display Broadway privado e um barramento D-Bus isolado. Valida senha com espaços, buffer protegido, cancelamento, expiração e progresso responsivo, sem conta. O teste NSS chama a implementação Rust e usa um verificador separado, também Rust, para preparar a base sintética e conferir a chave sem callback de senha.

A UI está em uma crate separada para que GTK/GIO não sejam ligados ao módulo PKCS#11 carregado no navegador. O módulo usa `-Bsymbolic-functions` no Linux para manter sua tabela ligada às próprias funções, evitando interposição de `C_GetFunctionList` por bibliotecas do host.

`cargo xtask chromium-tls-smoke --client /cliente --module /biblioteca` roda somente num contêiner descartável, como usuário normal, com Chromium nativo e `certutil` disponíveis. Usa uma CA de servidor local, carteira sintética e auxiliar compilado que assina com o fixture público. O servidor exige e verifica o certificado do cliente em TLS 1.2, sem ignorar erros de certificado. A política de seleção automática de certificado é exclusiva desse contêiner de laboratório; não é instalada pelo Axia. O teste passou com sandbox ativo e rede externa desabilitada. Isso não valida conta Certisign, GERID no Chromium, derivados, outras distribuições ou TLS 1.3. `--no-sandbox` permanece como opção de diagnóstico de contêiner, não utilizada na validação que passou.

`cargo xtask rpm-runtime-smoke` exige contêiner descartável, usuário normal e instalação RPM existente. Cria conta e perfil sintéticos novos, valida hardlink/ELF, dependências, migração NSS, backup e autostart, e salva um snapshot privado. Após reinstalar e remover o pacote, `cargo xtask rpm-runtime-smoke --check-preserved /home/usuario-de-teste` confere a preservação desses arquivos. A CI usa o ELF de desenvolvimento diretamente, sem precisar executar Cargo como o usuário de teste.

O aplicativo não executa Python ou Zenity. No Fedora 44, GTK depende de GStreamer, cujo pacote exige Python para ferramentas próprias; o gerenciador pode instalar esse interpretador indiretamente. Isso é uma dependência do toolkit do sistema, e não uma rotina Python do Axia. Não desativar nem remover o Python do sistema na máquina do usuário; a indisponibilidade do interpretador é testada somente em contêiner descartável.
