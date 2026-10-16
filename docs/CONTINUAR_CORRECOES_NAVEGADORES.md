# Continuidade: validar as seis correções dos navegadores

Atualizado em **15/10/2026**, depois do commit do ponto 6 em `cbdef29`. Os seis pontos estão commitados. O usuário solicitou retirar a instalação antiga e publicar a alpha.2 para uma instalação manual limpa. README e guia apontam para `v0.1.0-alpha.2`; confira a release no GitHub ao retomar. Este é o ponto de entrada para continuar a tarefa em outro computador. Leia também o código atual, os documentos das seis correções e `DEVELOPMENT.md`. O documento geral `../CONTINUIDADE_IA_FEDORA.md` conserva histórico anterior; as prioridades desta tarefa estão aqui.

**Depois da publicação:** a release alpha.2 foi publicada sobre `19da049`. O teste manual confirmou cadastro, carteira e integração no perfil principal, mas revelou um perfil adicional sem base NSS. Uma correção posterior separa inventário e elegibilidade para configuração, mantendo perfis secundários inicializados e omitindo os ainda vazios com uma orientação. Essa correção está local, sem commit ou nova release; veja a abertura de `../CONTINUIDADE_IA_FEDORA.md` e a seção de configuração de `DEVELOPMENT.md` antes de retomar. Não repetir a desinstalação nem zerar o cadastro do usuário.

## Combinado com o usuário

- Implementar **um ponto completo por vez**, na ordem 4 → 5 → 6.
- Concluir implementação, regressões e documentação; preparar somente as alterações daquele ponto e sugerir a mensagem de commit.
- **O usuário executa os commits das correções.** Esses commits já foram feitos até o ponto 6. A preparação e publicação da release alpha.2 foram autorizadas posteriormente, incluindo guia e link de download; não estender essa autorização a novos ciclos de desenvolvimento.
- O trabalho atual está no Fedora. O usuário decidiu fazer a validação manual no **Fedora e GNOME depois de concluir os seis pontos**. Continuar a validação automatizada sem exigir esse teste manual a cada ponto.
- O aplicativo detecta navegadores já instalados. **Não instalar navegadores** como requisito ou efeito do projeto.
- A configuração automática acontece quando o usuário clica em **Configurar navegadores**, escolhe os alvos e confirma. Não transformar isso em alterações silenciosas durante a instalação ou a inicialização.
- Incluir os derivados Firefox e Chromium cujo suporte seja fundamentado no código e nos metadados de instalação/perfil. Não prometer compatibilidade universal com todos os derivados.

## Estado confirmado ao escrever este documento

O ponto 5 foi commitado e enviado em `b023562`. O ponto 6 começou com o diretório de trabalho limpo sobre essa revisão e foi commitado e enviado pelo usuário em `cbdef29`:

| Ponto | Situação | Commit | Documento |
| --- | --- | --- | --- |
| 1. Seleção consistente do runtime nativo | Concluído | `39a3fac` | [FIX_01_RUNTIME.md](FIX_01_RUNTIME.md) |
| 2. Chromium aberto e recuperação da base NSS | Concluído | `9e695c9` | [FIX_02_CHROMIUM.md](FIX_02_CHROMIUM.md) |
| 3. Tela após cadastro salvo e consulta de carteira falha | Concluído | `b0667ca` | [FIX_03_CONNECTION.md](FIX_03_CONNECTION.md) |
| 4. Perfis secundários Firefox | Concluído | `488ed1a` | [FIX_04_FIREFOX_PROFILES.md](FIX_04_FIREFOX_PROFILES.md) |
| 5. Derivados Firefox e executáveis fora de `/usr/bin` | Concluído | `b023562` | [FIX_05_BROWSER_DISCOVERY.md](FIX_05_BROWSER_DISCOVERY.md) |
| 6. Resultados por alvo no painel | Concluído | `cbdef29` | [FIX_06_BROWSER_RESULTS.md](FIX_06_BROWSER_RESULTS.md) |

A migração dos testes Python para Rust também está no histórico atual. Não recriar a suíte Python. O fixture público `python-vectors.json` é intencional e permanece como referência sintética; não alterar seu conteúdo nem relaxar a exceção de hash para publicar outros arquivos privados.

Os seis pontos e a conciliação estão no histórico remoto. A publicação da alpha.2 e a instalação limpa foram solicitadas posteriormente pelo usuário; essa autorização não inclui executar login, cadastro ou assinatura real automaticamente.

## O que o projeto faz

Axia RemotedID é uma reimplementação independente em Rust, guiada pelo protocolo recuperado do DesktopID. Não é recompilação do C++ original. O certificado do usuário é em nuvem, utilizado com o aplicativo Certisign no celular. Cadastro, carteira, autorização e assinatura remota já possuem implementação; não reiniciar a engenharia reversa para resolver estes pontos da interface.

- `crates/desktopid-client`: protocolo, armazenamento, fluxo de cadastro, carteira e cache de sessão, sem GTK.
- `crates/desktopid-pkcs11`: provedor carregado por navegadores/aplicações, sem dependência GTK.
- `crates/axia-desktop`: executável `desktopid-client`, CLI, GTK, bandeja, instalação e configuração NSS.
- `crates/axia-devtools`: testes e empacotamento compilados, acessados por `cargo xtask`.

Os serviços desejados incluem Gerid, PJe, eproc, Projudi, PJeOffice, e-CAC, autenticação em sites e assinatura de PDF. Essa é a meta de compatibilidade, não uma lista de integrações integralmente validadas. Assinaturas reais de laboratório via CLI e PKCS#11 já foram confirmadas pelo usuário anteriormente; não são uma pendência que exija repetir operações reais agora.

## Ponto 4 — listar os perfis secundários Firefox

### Defeito corrigido no ponto 4

A implementação anterior enumerava os perfis de `profiles.ini`, mas, quando encontrava perfis preferidos, retornava apenas esses e descartava os demais. A descoberta agora está em `browsers/firefox.rs` e mantém todos os perfis existentes. Exemplo corrigido: **Principal** é o padrão e **Trabalho** também aparece para configuração.

### Implementação esperada

1. Enumerar todos os perfis válidos já existentes, sem criar perfis de navegador.
2. Preservar a informação de preferência/padrão como metadado e/ou indicação na apresentação. Separar a descoberta de todos os perfis da escolha de um alvo padrão.
3. Deduplicar pelo caminho canônico. Conservar os controles existentes de caminho, propriedade, bloqueio e backup na configuração.
4. Distinguir visualmente nomes iguais com informação suficiente sobre o perfil, sem registrar dados pessoais em logs públicos. Rever a estrutura `Target` e a apresentação em `setup_ui`/`ui::choose` se necessário.
5. Rever `Inventory::firefox_target()` e o comando legado `setup-firefox`: adicionar perfis secundários não deve passar a escolher arbitrariamente o primeiro. Um padrão inequívoco pode ser utilizado; múltiplos padrões de instalação exigem seleção explícita, e `--profile` deve continuar funcionando.
6. Manter a seleção gráfica explícita e o cancelamento sem alterações. Não usar a presença de um perfil secundário como consentimento para modificá-lo.

### Critérios de aceitação

- Principal e Trabalho aparecem quando Principal é o padrão.
- Perfis sem padrão também aparecem; múltiplos padrões permanecem distinguíveis.
- Nome repetido, duas raízes Firefox/XDG e referências duplicadas ao mesmo diretório não produzem escolhas ambíguas ou registros duplicados.
- Caminhos relativos/absolutos e diretórios ausentes são tratados sem inventar perfis.
- Configurar um perfil selecionado não altera os não selecionados; cancelar não abre NSS nem altera a base.
- Os testes existentes que esperam somente o padrão devem ser atualizados para a nova especificação, mantendo a proteção contra adivinhar entre instalações diferentes.

Arquivos principais: `browsers/firefox.rs`, `browsers.rs` (`Target`, `Inventory::firefox_target`, `discover_at`, `setup_with`), `ui.rs` (`choose`) e `main.rs` (`SetupFirefox`, `SetupBrowsers`). Este ponto está implementado; o comportamento e o contrato JSON estão em [FIX_04_FIREFOX_PROFILES.md](FIX_04_FIREFOX_PROFILES.md). Parar para o commit antes do ponto 5.

Mensagem de commit sugerida, ajustável ao resultado: `fix: inclui perfis secundários do Firefox na configuração`.

## Ponto 5 — derivados Firefox e outras localizações de instalação

### Defeito corrigido no ponto 5

A descoberta anterior fixava `/usr/bin` e só reconhecia Firefox nessa família. Agora `discovery::Search` considera PATH e metadados desktop, e o catálogo Gecko inclui LibreWolf com seus próprios perfis. A descoberta Chromium continua usando os mesmos aliases da proteção de processos. O escopo implementado, as fontes e o contrato JSON estão em [FIX_05_BROWSER_DISCOVERY.md](FIX_05_BROWSER_DISCOVERY.md). Zen e Floorp não foram acrescentados nesta entrega.

### Implementação esperada

1. Criar um catálogo verificável para os derivados que serão efetivamente suportados: nome, identificadores, comandos/lançadores, raízes de perfis e forma de instalação.
2. Conferir caminhos e formatos em fontes primárias de cada navegador. LibreWolf é um caso de aceitação; outros derivados, como Floorp ou Zen, precisam de evidência antes de anunciar suporte.
3. Descobrir executáveis em localizações apropriadas, incluindo `PATH` e/ou metadados desktop quando justificado. Considerar instalações no usuário e lançadores que apontam a executáveis em `/opt`. Não executar comandos arbitrários extraídos de arquivos `.desktop` para detectar o navegador.
4. Reutilizar a descoberta de perfis corrigida no ponto 4, preservando qual navegador/instalação é dono de cada alvo e evitando colisões entre raízes.
5. Conferir associação entre executável, instalação e perfis. Um diretório residual sozinho não comprova que um navegador utilizável esteja instalado.
6. Tratar Flatpak como instalação separada e não validada. Não configurar automaticamente uma base do host supondo que será visível no sandbox. Não ampliar suporte a Snap/AppImage sem análise própria.
7. Manter a regra: o projeto configura os navegadores existentes e **não instala navegador**. Atualizar avisos e documentação para descrever somente a cobertura demonstrada.

### Critérios de aceitação

- Fixture de Firefox nativo continua sendo descoberto.
- Fixture de LibreWolf nativo, inclusive fora de `/usr/bin`, enumera seus próprios perfis.
- Nomes iguais de perfis em navegadores diferentes permanecem identificáveis.
- Duplicatas de `PATH`, symlinks e lançadores não duplicam alvos nem associam a base de outro produto.
- Instalação ausente, perfil ausente e somente Flatpak produzem avisos corretos, sem modificar configurações.
- A proteção contra Chromium aberto continua cobrindo todos os aliases reconhecidos pela descoberta.
- Testes de descoberta usam diretórios/executáveis simulados; não instalar navegadores para fazer esses testes passarem.

Mensagem de commit sugerida: `feat: detecta LibreWolf e instalações nativas fora de /usr/bin`.

## Ponto 6 — resultados estruturados e estado visível no painel

### Defeito corrigido no ponto 6

O worker convertia cada `register()` em texto e `setup_with()` retornava uma instrução genérica para reabrir os navegadores mesmo quando todos falhavam. O painel não recebia os resultados por alvo. Agora um relatório estruturado alimenta uma área própria no painel, preservada durante a sessão da janela, sem interferir no estado de cadastro/carteira da correção 3. Causas e recuperação usam tipos próprios; nova tentativa atualiza o perfil correto. O contrato JSON do lote CLI foi versionado intencionalmente. Comportamento e regressões em [FIX_06_BROWSER_RESULTS.md](FIX_06_BROWSER_RESULTS.md).

### Implementação esperada

1. Retornar um relatório estruturado por alvo: identidade do navegador/perfil, situação e informações úteis do registro. Distinguir sucesso, falha e necessidade de fechar o navegador; representar cancelamento/ausência de alvos sem confundi-los com sucesso.
2. Conservar o sucesso de um alvo mesmo quando outro falha. Não interromper ou resumir toda a operação como sucesso apenas porque o worker terminou sem erro de infraestrutura.
3. Atualizar o painel principal após a ação, deixando os resultados acessíveis depois de fechar a janela de mensagem. Não basta trocar a mensagem genérica de um popup.
4. Exibir instrução para reabrir navegadores somente quando houver alvos configurados. Se todos falharem, mostrar como corrigir/repetir a ação; se houver resultado parcial, distinguir os casos.
5. Preservar causas úteis dos erros NSS e orientações de fechar navegador/backup preservado, sem expor corpos do serviço, credenciais ou dados privados. Preferir tipos de resultado a classificação baseada apenas em substrings de mensagens.
6. Definir explicitamente a duração do estado: no mínimo, deve continuar visível durante a sessão do painel. Se optar por persistência entre execuções, justificar o formato, manter dados privados fora do Git e revalidar antes de apresentar um resultado antigo como configuração atual.
7. Não misturar este relatório com autorização de assinatura, estado do cadastro ou instalação de navegador. Conservar a atualização da carteira e dos controles implementada no ponto 3.

### Critérios de aceitação

- Sucesso total, falha total, sucesso parcial, navegador aberto, cancelamento e nenhum alvo têm resultados distintos.
- O painel mostra quais alvos foram configurados e quais precisam de ação; uma falha não apaga os sucessos dos demais.
- Uma nova tentativa atualiza o alvo correto sem duplicar o registro NSS.
- Cancelar não cria backup nem inicia registro.
- O resultado de rollback suspenso do ponto 2 não é apresentado como base restaurada.
- Testes GTK usam display privado e dados sintéticos, sem perfis ou conta reais.

Arquivos principais: `browsers.rs` (`Registration`, `register`, `setup_with`, `setup_ui`), `gui.rs`, `ui.rs` e eventuais saídas CLI afetadas. Preserve ou documente alterações no contrato JSON da CLI.

Mensagem de commit sugerida: `feat: mostra resultados de configuração por navegador no painel`.

## Regressões que não podem ser perdidas

- Seleção consistente do par módulo/auxiliar nativo; RPM legado não prevalece sobre instalação atual. O identificador de compatibilidade não é uma assinatura de autenticidade. Reinstalar módulo e auxiliar juntos ao testar uma build nova.
- Migração somente dos registros reconhecidos do Axia, com backup privado, rollback, idempotência e preservação de módulos de terceiros.
- Firefox usa sua trava `fcntl`. A trava `.axia-integration.lock` do Chromium coordena somente o Axia; não impede uma abertura externa do navegador. Manter as verificações repetidas de processos e a proteção do rollback.
- Cadastro salvo com consulta de carteira falha deixa **Atualizar certificados** disponível na mesma janela. Não recuperar a antiga flag de cadastro da GUI.
- Tentativa de cadastro ambígua não autoriza novo cadastro. Não apagar `registration*.json`, chave ou carteira para contornar esse bloqueio.
- Cancelamento, ausência de conta e inventário público não iniciam operações reais.
- Módulo PKCS#11 continua sem GTK e sem carregar a chave privada da instalação apenas para enumerar certificados.
- Preferência atual: token de sessão somente em memória, com limite de dez minutos no mesmo contexto. Não alterar essa decisão para resolver descoberta de navegador.

## Como retomar em outro computador

Repositório: `https://github.com/rodolfoaltenrath/axia-remoteid`.

Antes de sair do computador atual, salvar este documento e o link no documento geral em um commit e enviar os commits pelo fluxo Git habitual. **Copiar só este Markdown não transfere as implementações.** No outro computador, clonar o repositório ou atualizar um checkout limpo com `git pull --ff-only`. Se houver mudanças locais, inspecioná-las antes; não usar reset/clean para descartá-las.

Conferir `git status --short` e `git log -4 --oneline`. O histórico precisa conter `b0667ca` ou commits posteriores que o incluam. Os hashes são referências do estado desta entrega, não uma exigência de permanecer nesse HEAD para sempre.

Não é necessário transferir gigabytes de `target`, `dist`, `.local-research` ou `test-results`; esses diretórios são ignorados e reconstruíveis. Os testes usam fixtures públicos. **Não enviar identidade, carteira privada ou credenciais ao GitHub.** Uma nova máquina pode desenvolver os pontos 4 a 6 sem configurar uma conta real.

No Windows, se Fedora WSL já estiver instalado:

```powershell
wsl.exe --list --verbose
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\test_rust_wsl.ps1 -Distro FedoraLinux-44
```

Conferir o nome real da distribuição e ajustar `-Distro`. Para preparar dependências de compilação pelo wrapper, existe `-Prepare`; o teste GTK também exige Broadway/D-Bus e o teste PKCS#11 exige OpenSC/NSS, conforme as dependências Linux abaixo. Não assumir que a identidade ou o WSL do computador anterior estarão presentes.

No Fedora nativo ou dentro do Fedora WSL, executar na raiz do checkout:

```bash
# Se as dependências ainda não estiverem instaladas:
sudo dnf install git cargo rust gcc openssl-devel gtk4-devel clippy rustfmt dbus-daemon opensc nss desktop-file-utils util-linux

# Dentro de WSL como root, omitir sudo somente nessa preparação do ambiente.
# Usar um diretório de compilação Linux próprio deste checkout.
export CARGO_TARGET_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/axia-remoteid-continuacao-target"

sh scripts/test_rust_wsl.sh
cargo xtask native-ui-smoke --target "$CARGO_TARGET_DIR"
cargo xtask check-publication
```

Não compartilhar o mesmo `CARGO_TARGET_DIR` entre checkouts com fontes diferentes: já houve reaproveitamento de um executável de teste antigo nesse cenário. Uma contagem inesperada exige investigar/recompilar os artefatos afetados, sem alterar fontes ou desativar testes para acomodar o cache.

Quando o ponto alterar descoberta/registro NSS, executar também:

```bash
cargo xtask firefox-nss-smoke \
  --module "$CARGO_TARGET_DIR/debug/libdesktopid_pkcs11.so" \
  --client "$CARGO_TARGET_DIR/debug/desktopid-client"
```

Quando houver mudança no módulo, auxiliar ou contrato PKCS#11, usar `sh scripts/test_pkcs11_wsl.sh`. Os testes Chromium TLS e de instalação RPM exigem contêiner descartável; não instalar um navegador na máquina do usuário só para satisfazer esses testes. Os comandos e a CI estão em `DEVELOPMENT.md` e `../.github/workflows/ci.yml`.

O teste GTK ignorado da suíte comum é executado por `native-ui-smoke` em Broadway/D-Bus privados. Ele já chama `gui::connection_tests::exercise_connection_recovery()` na mesma thread GTK; manter esse cuidado ao acrescentar testes de painel, evitando inicializar GTK em threads de testes diferentes.

## Última validação realizada

Após o ponto 6 passaram **140 testes Rust comuns**, incluindo cinco regressões novas do relatório, e **21 verificações do executável**. O teste GTK isolado inclui a área de resultados, atualização do perfil correto, preservação dos demais, cancelamento, erro, restauração suspensa e duração da sessão; mantém os cenários de seleção, rolagem e recuperação do cadastro. Formatação, Clippy com `-D warnings`, builds debug/release e verificação de publicação passaram. Publicação: **98 fontes**, nenhuma ocorrência. Os relatórios locais são `test-results/fix06-*`; são ignorados e podem não existir no outro computador.

Também passou novamente a integração Firefox/NSS com carregamento real, migração do RPM antigo, backup, rollback, idempotência, conflito e preservação de um segundo perfil não selecionado. O cenário novo de lote CLI confirmou falha total restaurada, sucesso parcial apesar de falha no primeiro alvo, correção e repetição sem duplicar o registro existente. Esses resultados não homologam acesso real em LibreWolf. Não fixar a contagem de testes no código do relatório.

As seis correções não acessaram conta real, cadastraram computador ou solicitaram assinatura real. A validação manual no Fedora GNOME deve conferir, como usuário normal, descoberta dos navegadores existentes, seleção dos perfis, cancelamento, bloqueio com navegador aberto, repetição idempotente e resultados no painel. Ao testar a nova build, atualizar módulo e auxiliar como um par nativo; a instalação antiga do usuário não foi substituída pelos testes. Qualquer autenticação/assinatura real será uma ação posterior consciente do usuário, não parte automática dos testes de desenvolvimento.

## Instrução curta para a próxima IA

> Leia este documento, `CONTINUIDADE_IA_FEDORA.md`, os seis documentos `docs/FIX_*.md` e o código atual. As seis correções estão implementadas e commitadas até `cbdef29`. Verifique o estado do Git e a release `v0.1.0-alpha.2` antes de prosseguir. Solicitei retirar a instalação antiga e publicar essa versão para baixar e configurar manualmente do zero. Preserve as cópias privadas fora do Git. A próxima etapa é validar a instalação e o fluxo real no Fedora/GNOME. Não recrie implementações concluídas, não instale navegadores nem execute autenticações ou assinaturas reais automaticamente. Novos commits e publicações fora da preparação desta release exigem instrução minha.
