# Correção 3 — recuperação da conexão após cadastro salvo

Implementada em 15/10/2026. Escopo: recuperar o painel quando o cadastro foi persistido e a consulta da carteira falhou. Os pontos 4 a 6 da revisão permanecem pendentes.

## Problema e comportamento atual

O backend salva o código de cadastro antes de consultar os certificados. A tela mantinha uma flag de cadastro anterior à operação e só a atualizava após sucesso completo. Se a consulta falhasse, o próximo clique tentava novamente o caminho de cadastro, que era corretamente bloqueado pelo backend porque a instalação já tinha código salvo.

A tela agora lê o estado persistido antes de escolher a ação do clique e depois de qualquer resultado da operação, incluindo erro e cancelamento. Não mantém essa flag entre operações.

- Cadastro confirmado e carteira indisponível: mostra **Atualizar certificados**, informa que o cadastro está salvo e permite uma nova consulta na mesma janela. Essa consulta reutiliza o código e a identidade, sem pedir credenciais nem cadastrar novamente.
- Carteira ausente ou inválida: **Configurar navegadores** permanece desabilitado até haver uma carteira local validada.
- Atualização concluída: atualiza o resultado e libera a configuração de navegadores quando a carteira local passa na validação.
- Erro de atualização com carteira válida anterior: conserva a disponibilidade dos certificados salvos e permite consultar novamente.
- Cadastro ainda não confirmado e tentativa anterior ambígua: conserva a proteção contra novo cadastro. Não transforma uma falha desconhecida em autorização de repetição.
- Identidade/configuração ilegível após a operação: informa o erro e desabilita as ações de conexão e configuração, em vez de supor que a instalação nunca foi cadastrada.

Não há repetição automática de login, cadastro ou consulta. O usuário inicia cada atualização. A correção não modifica protocolos, credenciais persistidas, autorização de assinatura nem os resultados de configuração por navegador previstos no ponto 6.

## Validação automatizada

Um teste do backend usa servidor HTTP de loopback sintético: login aceito, cadastro aceito, carteira HTTP 500, nova consulta HTTP 503 e consulta seguinte bem-sucedida. Verifica uma única chamada de login, um único cadastro, três consultas de carteira e preservação dos arquivos de identidade e do journal. A resposta de erro sintética não aparece na mensagem pública. O suporte HTTP é privado do módulo de testes; a aplicação continua exigindo HTTPS.

Os testes de `axia-desktop/src/gui/connection_tests.rs` verificam a escolha de atualização com cadastro salvo e sem carteira, cache inválido, cancelamento sem criar identidade, tentativa ambígua e estado local ilegível.

O teste GTK existente, executado em Broadway e D-Bus privados, também exercita um worker que salva um cadastro sintético e retorna erro de consulta. Confere o texto e a disponibilidade dos botões na mesma instância, uma nova falha, a recuperação da carteira, a preservação de cache válido após erro e a recusa quando o estado local fica ilegível. Usa a mesma thread GTK do teste anterior.

```bash
sh scripts/test_rust_wsl.sh
cargo xtask native-ui-smoke --target "${CARGO_TARGET_DIR:-$HOME/.cache/desktopid-rust-target}"
```

Tudo usa diretórios temporários e dados de laboratório, sem conta Certisign real. A validação manual no Fedora GNOME permanece para depois das seis correções, conforme combinado.
