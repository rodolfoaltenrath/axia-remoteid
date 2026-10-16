# Correção 4 — perfis secundários do Firefox

Implementada em 15/10/2026 sobre `29d9b60`. Os pontos 5 e 6 continuam pendentes. Não houve instalação de navegador, acesso à conta real ou publicação de build.

## Comportamento

A descoberta lista todos os diretórios de perfil existentes referenciados em `profiles.ini`, tanto na raiz XDG quanto em `~/.mozilla/firefox`. Referências duplicadas são reunidas pelo caminho canônico, inclusive aliases por symlink, caminhos absolutos e referências de duas raízes. Diretórios ausentes, caminhos vazios, arquivos comuns e caminhos relativos marcados como absolutos não viram alvos nem são criados.

Perfis marcados em `[Install…]` aparecem como **padrão de instalação**; a marca `Default=1` de uma seção de perfil aparece como **padrão geral**. Essa distinção segue a [documentação do serviço de perfis da Mozilla](https://firefox-source-docs.mozilla.org/toolkit/profile/changes.html). Ela serve para apresentar e escolher padrões, sem excluir os perfis secundários.

O diálogo mostra também a localização de cada perfil, abreviada com `~/` quando fica no home do usuário. Isso distingue nomes e diretórios finais iguais, inclusive em raízes diferentes. O caminho é apresentado na seleção local e não é acrescentado ao rótulo usado nas mensagens de progresso. O JSON de inventário já inclui o caminho em `database`.

A ordem visual conserva a ordem da descoberta, com os padrões primeiro. Textos longos quebram linha, e uma área com rolagem mantém os botões acessíveis quando há muitos perfis. A pessoa confirma os perfis selecionados; cancelar ou deixar a seleção vazia não inicia o registro NSS. Configurar um perfil não altera os demais. As verificações de propriedade, permissões, trava Firefox e backup do registro continuam no mesmo caminho de produção.

## Terminal e contrato JSON

`setup-browsers --list` continua sendo somente leitura e agora inclui os perfis secundários. Os campos anteriores de cada alvo permanecem. Há um campo opcional novo:

```json
{"firefox_default":"installation"}
```

O valor `profile` identifica o padrão geral. O campo é omitido em perfis sem marca de padrão e no alvo Chromium. Os rótulos dos padrões recebem a indicação correspondente; consumidores devem usar `database` para identificar o alvo, não o texto de apresentação.

`setup-firefox --yes` sem `--profile` escolhe um único padrão de instalação. Na ausência dessa marca, pode escolher um único padrão geral; sem nenhuma marca, aceita somente um único perfil disponível. Múltiplos padrões de instalação, múltiplos padrões gerais ou múltiplos perfis sem padrão exigem seleção explícita. Uma marca geral não resolve a ambiguidade entre dois padrões de instalação. `--profile` continua definindo o alvo explicitamente.

`setup-browsers --yes` confirma a configuração de **todos** os alvos detectados, agora incluindo os perfis secundários; `--family firefox` restringe a família, não escolhe um perfil. Para selecionar individualmente, use a interface ou `setup-firefox --yes --profile …`.

## Validação

As novas regressões em `browsers/firefox/tests.rs` cobrem padrão de instalação e geral com perfis secundários, padrões ambíguos, ausência de padrão, nomes repetidos, duas raízes, symlinks, deduplicação, caminhos relativos/absolutos e ausência de criação de perfis. O controlador real é exercitado com fronteiras gráficas/NSS sintéticas para verificar cancelamento e passagem exclusiva do perfil secundário selecionado ao worker.

O teste GTK isolado verifica ordem visual, texto com nome/localização, quebra de linha, seleção apenas do segundo perfil, cancelamento e rolagem com 30 perfis sintéticos. Usa a mesma thread GTK dos testes de formulário e recuperação da conexão. `firefox-nss-smoke` mantém uma segunda base NSS real de laboratório e confere que seus bytes e arquivos não mudam enquanto o perfil selecionado passa por registro, repetição, migração, falha e rollback.

```bash
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --locked
cargo xtask native-ui-smoke --target "${CARGO_TARGET_DIR:-target}"
cargo xtask firefox-nss-smoke \
  --module "${CARGO_TARGET_DIR:-target}/debug/libdesktopid_pkcs11.so" \
  --client "${CARGO_TARGET_DIR:-target}/debug/desktopid-client"
cargo xtask check-publication
```

Tudo usa dados sintéticos e diretórios temporários. A validação manual no Fedora/GNOME continua prevista para depois do ponto 6.

Nesta entrega passaram **117 testes Rust comuns**, o teste GTK isolado, **19 verificações CLI**, Firefox/NSS, formatação, Clippy com `-D warnings` e builds debug/release. A publicação examinou **90 fontes** sem ocorrências. Relatórios em `test-results/fix04-*`, ignorados pelo Git. A nova verificação CLI inclui `setup-browsers --list` sem display, conta ou criação de perfis.
