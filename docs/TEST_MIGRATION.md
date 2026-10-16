# Cobertura da migração dos testes para Rust

As 26 regressões da antiga suíte Python foram substituídas ou incorporadas aos testes de produção Rust. Os dez arquivos Python, `requirements-test.txt` e o job `python-reference` foram removidos. As implementações antigas continuam disponíveis no histórico Git, anterior a esta migração.

## Protocolo: 14 cenários

| Cenários anteriores | Verificação atual |
| --- | --- |
| Ordenação e booleanos; objetos/arrays com ID zero; UTF-8 sem escapes; recusa de arrays escalares; limite int64 e recusa de floats (5) | `recovered_canonicalization_matches_literal_wire_bytes` e `unsupported_json_cannot_be_signed`, em `crates/desktopid-client/tests/protocol.rs`: bytes literais, extremos int64 e arrays de números, strings, booleanos e null |
| PIN/OTP com zeros; digest bruto e tamanho correto (2) | `request_keeps_leading_zeroes_and_sends_digest_only`, incluindo igualdade do payload completo |
| Respostas inválidas; preservação de todos os bytes e diferentes formatos de ID (2) | `signature_response_requires_one_successful_well_formed_result` e `response_id_is_metadata_and_does_not_have_to_echo_numeric_zero`, incluindo os 256 valores de byte |
| Cabeçalho RSA/SHA-256 e rejeição de documento modificado (1) | `authorization_has_exact_rsa_encoded_message_and_rejects_changed_body`: exponenciação modular, codificação RSA literal e verificador OpenSSL independente; há também vetores congelados e testes de adulteração |
| Série antes do emissor; compressão de separadores; recusa de referência ambígua/Unicode; prazo recuperado de 1800 segundos (4) | `key_name_*` e `session_cache_lifetime_matches_recovered_token_store`; o cache operacional de dez minutos tem testes próprios |

## Firefox e instalação: 6 cenários

| Cenários anteriores | Verificação atual |
| --- | --- |
| Preferência de instalação acima do perfil legado; vários padrões exigem seleção explícita (2) | `discovery_detects_both_families_and_respects_existing_chromium_database` e `multiple_firefox_install_defaults_require_explicit_selection`; o último também exercita a escolha usada pela CLI |
| Trava Firefox mantida por outro processo; recusa de symlink (2) | `firefox_profile_lock_held_by_another_process_is_preserved`: outro processo Rust mantém um `F_SETLK` independente do auxiliar de produção; `profile_lock_refuses_symlinks_without_touching_the_target` preserva o destino |
| Atalho com espaços/metacaracteres; recusa de substituição de symlink (2) | `native_install_preserves_account_and_refuses_foreign_launcher`, com `desktop-file-validate`, permissões e preservação da identidade |

O teste de múltiplos padrões não representa suporte a todos os perfis secundários. A ampliação da descoberta continua sendo o ponto 4 do [plano de navegadores](CONTINUAR_CORRECOES_NAVEGADORES.md).

## Fluxo desktop/RPM: 6 cenários

| Cenários anteriores | Verificação atual |
| --- | --- |
| Migração apenas dos próprios atalhos e alias (1) | `migration_hides_only_our_legacy_entry_updates_alias_and_is_idempotent`: entradas e aliases de terceiros, ações secundárias e idempotência |
| Cancelar configuração; impedir configuração sem carteira (2) | `setup_with` de produção com diálogo/worker injetados: `cancelled_empty_or_invalid_browser_selection_preserves_profile_and_never_registers` e `browser_setup_without_account_never_prepares_or_registers_a_profile`; nenhuma chamada de registro nesses casos |
| CLI não inicia interface ou configura navegador (1) | `cargo xtask cli-smoke`: ajuda/versão sem criar estado, preferências ou perfis, além dos testes headless existentes |
| Recusa de conta gráfica como root (1) | O teste GTK `native_forms_clear_passwords_cancel_timeout_and_keep_worker_responsive` chama a GUI como root no contêiner da CI e verifica a recusa sem criar identidade |
| Migração NSS explícita com backup (1) | `installation/runtime/tests.rs`, `firefox-nss-smoke` e `rpm-runtime-smoke`, incluindo migração do registro legado, backup privado, rollback e reinstalação/remoção em contêiner |

## Execução e independência das expectativas

`cargo test --workspace --locked` executa as regressões comuns, incluindo o teste que inicia um segundo processo para manter a trava Firefox. `cargo xtask native-ui-smoke` executa o teste GTK ignorado na suíte comum em Broadway e D-Bus privados. Os demais testes usam os comandos documentados no [guia de desenvolvimento](DEVELOPMENT.md).

`python-vectors.json` é um conjunto JSON imutável de resultados externos: sua chave privada RSA é sintética, publicada deliberadamente e não pertence a uma conta. Seu conteúdo e a exceção de publicação por SHA-256 foram preservados, inclusive a normalização LF/CRLF. O nome e o texto histórico assinado registram a origem dos dados; não são código Python. A geração de novos fixtures Rust não substitui automaticamente esses resultados. Os testes comparam expectativas literais, vetores fixos, decodificação RSA por matemática modular e interoperabilidade real com NSS/OpenSC.

O verificador de publicação também recusa novas fontes Python, bytecode e manifestos de dependências Python. A CI não instala nem chama Python diretamente. GTK/GStreamer do Fedora pode manter uma dependência indireta do interpretador; essa dependência do sistema não é removida nesta migração.
