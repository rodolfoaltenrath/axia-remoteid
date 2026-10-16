# Correção 5 — navegadores e locais de instalação

Implementada sobre `488ed1a`. Esta etapa acrescenta LibreWolf nativo e a busca de navegadores fora de `/usr/bin`, sem instalar ou abrir navegador. O ponto 6, resultados por alvo no painel, continua pendente.

## Catálogo e fontes

| Produto | Comandos | Identificadores desktop adicionais | Perfis / certificados |
| --- | --- | --- | --- |
| Firefox | `firefox` | `org.mozilla.firefox.desktop` | `${XDG_CONFIG_HOME:-~/.config}/mozilla/firefox` e `~/.mozilla/firefox` |
| LibreWolf | `librewolf` | `io.gitlab.librewolf-community.desktop` | `~/.librewolf` quando esse diretório existe; caso contrário, `${XDG_CONFIG_HOME:-~/.config}/librewolf/librewolf` |
| Chromium | `chromium`, `chromium-browser` | `org.chromium.Chromium.desktop` | Base NSS compartilhada do usuário |
| Google Chrome | `google-chrome`, `google-chrome-stable` | `com.google.Chrome.desktop` | Base NSS compartilhada do usuário |
| Brave | `brave-browser`, `brave-browser-stable` | `com.brave.Browser.desktop` | Base NSS compartilhada do usuário |
| Microsoft Edge | `microsoft-edge`, `microsoft-edge-stable` | — | Base NSS compartilhada do usuário |
| Vivaldi | `vivaldi`, `vivaldi-stable` | — | Base NSS compartilhada do usuário |
| Opera | `opera` | — | Base NSS compartilhada do usuário |

Para todos os produtos, os nomes dos comandos com sufixo `.desktop` também são reconhecidos. Os aliases Chromium continuam definidos em `browsers/chromium.rs`, compartilhados com a proteção de processos; a descoberta não cria uma lista paralela de comandos.

A [FAQ oficial do LibreWolf](https://librewolf.net/docs/faq/#how-do-i-migrate-to-the-new-xdg-paths) documenta os caminhos e a precedência da raiz legada. A mesma FAQ apresenta o [lançador nativo com variáveis Wayland](https://librewolf.net/docs/faq/#cant-open-links-with-librewolf-when-using-wayland), usado como caso de teste. O formato `profiles.ini` e os padrões seguem a [documentação do serviço de perfis Mozilla](https://firefox-source-docs.mozilla.org/toolkit/profile/changes.html). A base compartilhada segue a [documentação NSS do Chromium](https://chromium.googlesource.com/chromium/src/+/main/docs/linux/cert_management.md).

Zen, Floorp e outros derivados não foram acrescentados ao catálogo desta entrega. Não anunciar compatibilidade automática com todos os derivados Firefox.

## Descoberta somente por leitura

A busca considera entradas absolutas de `PATH`, `~/.local/bin`, `/usr/local/bin`, `/usr/bin` e `/bin`. Entradas relativas e vazias de `PATH` são ignoradas. Um tarball extraído em um local arbitrário precisa ter um comando em um desses diretórios ou um lançador reconhecido; não há varredura recursiva do disco.

Os lançadores reconhecidos são procurados em `${XDG_DATA_HOME:-~/.local/share}/applications` e nos diretórios `applications` de `XDG_DATA_DIRS`, cujo padrão é `/usr/local/share:/usr/share`. O lançador do usuário prevalece sobre o mesmo ID do sistema, inclusive quando tem `Hidden=true`. Exportações desktop do Flatpak são examinadas apenas para identificação da forma de instalação.

O campo `Exec` é analisado como texto, sem executar shell, `env`, `TryExec`, navegador ou ferramenta de metadados. O programa precisa ter um nome do catálogo e estar disponível como arquivo executável. Caminhos absolutos entre aspas e as variáveis `GDK_BACKEND` e `MOZ_ENABLE_WAYLAND` antes do programa são aceitos. Alterações de HOME/XDG, seleção de perfil por argumento e comandos intermediários não reconhecidos não são interpretados como a instalação padrão. `TryExec`, quando presente, é verificado por leitura e permissão, sem execução. A análise é um subconjunto conservador da [especificação Desktop Entry](https://specifications.freedesktop.org/desktop-entry/latest/exec-variables.html), não um executor genérico de comandos desktop.

Arquivos e metadados têm leitura limitada. A descoberta aceita ELF e lançadores com shebang, confere permissões de execução para o processo e rejeita associações contraditórias de nome, `application.ini` ou encaminhamento literal para outro navegador. Essas verificações não são assinatura de autenticidade nem teste de funcionamento do navegador.

## Associação de perfis

O leitor do ponto 4 é reutilizado para ambos os produtos Gecko. Cada perfil conserva nome, marca de padrão e caminho canônico, com o nome do produto no rótulo e sua instalação nos metadados. Um diretório de perfis residual não produz alvos sem um executável compatível encontrado.

Múltiplos caminhos, symlinks e lançadores do mesmo executável não duplicam perfis. Quando existem duas instalações nativas do mesmo produto, seus executáveis são reunidos e seus perfis aparecem uma vez. Se dois produtos referenciam a mesma base física, os alvos conflitantes são omitidos com aviso, em vez de atribuir a base ao primeiro produto encontrado. As bases Chromium continuam compartilhadas em um único alvo, com as instalações encontradas relacionadas nesse alvo.

`setup-firefox --yes` sem `--profile` continua escolhendo somente um perfil Firefox inequívoco; não passa a escolher LibreWolf por ser o único produto Gecko disponível. Um conflito com outro produto também impede escolher automaticamente um perfil secundário no lugar do padrão omitido. `--profile` continua funcionando como alvo explícito. O diálogo permite escolher perfis Firefox e LibreWolf e mantém as verificações de propriedade, trava e backup do registro NSS.

## Contrato JSON

Os campos anteriores do inventário são mantidos. Cada alvo descoberto ganha `installations`, com uma instalação para Firefox/LibreWolf ou várias no alvo compartilhado Chromium:

```json
{"installations":[{"id":"librewolf","name":"LibreWolf","executables":["/opt/librewolf/librewolf"]}]}
```

`executables` contém os caminhos canônicos dos executáveis/lançadores reconhecidos. Esse dado é inventário local, assim como `database`; não é acrescentado aos rótulos de progresso. O campo é omitido em alvos manuais sem descoberta. `installed` continua sendo a lista de nomes de produtos nativos encontrados. `family: "firefox"` representa a família Gecko, inclusive LibreWolf; o produto é identificado em `installations`.

## Ambientes não validados

Flatpak, Snap e AppImage não ganham integração automática com a base do host. Marcadores desktop, caminhos e wrappers conhecidos são recusados ou sinalizados; AppImages reconhecidos pela extensão ou pelos marcadores do [formato oficial](https://github.com/AppImage/AppImageSpec/blob/master/draft.md#image-format) também são excluídos, inclusive quando renomeados. Não são concedidas permissões de sandbox ou modificadas configurações de isolamento.

O catálogo inclui apenas os aliases Chromium já cobertos pela detecção de processos. Um alias que resolva para um nome canônico desconhecido dessa proteção é recusado. O Axia não instala navegador, não transforma diretórios de configuração em prova de instalação e não inicia uma operação real para validar a descoberta.

## Validação

As regressões de `browsers/discovery/tests.rs` cobrem Firefox e LibreWolf simultâneos, precedência LibreWolf, perfis secundários, nomes repetidos entre produtos, PATH e desktop, espaços e aspas, deduplicação, instalações múltiplas, perfil compartilhado ambíguo, executável ausente/incompatível e ambientes isolados. Os testes do ponto 4 e da proteção Chromium continuam sendo executados.

`cli-smoke` também exercita o executável real com HOME/XDG privados, Firefox simulado no diretório do usuário e LibreWolf em outro local apontado por um desktop. Confere os perfis de cada produto, a ausência de execução dos lançadores e a preservação dos arquivos. Não exige instalar navegador para passar.

A descoberta é validada com fixtures; isso não homologa autenticação Certisign em LibreWolf ou em cada derivado Chromium. O teste manual no Fedora/GNOME continua previsto para depois do ponto 6. Não houve acesso à conta real, atualização da instalação do usuário ou publicação de RPM.

Resultados desta entrega: **135 testes Rust comuns**, **20 verificações CLI**, GTK isolado em três execuções consecutivas, Firefox/NSS, formatação, Clippy com `-D warnings` e builds debug/release aprovados. O teste GTK agora espera o layout com prazo limitado, conservando os critérios de rolagem/altura; a medição anterior após 50 ms falhou durante compilação concorrente. Os identificadores nativos foram confirmados no auxiliar e no módulo release. O verificador de publicação examinou **93 fontes**, sem ocorrências. Relatórios em `test-results/fix05-*`, ignorados pelo Git.
