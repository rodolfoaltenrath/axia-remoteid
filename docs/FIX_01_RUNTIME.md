# Correção 1 — seleção consistente do runtime nativo

Implementada em 15/10/2026. Escopo: impedir que a instalação nativa atual configure o navegador com a biblioteca ou o auxiliar da alpha.1. Os outros cinco itens da revisão do fluxo permanecem pendentes.

## Comportamento

O RPM só tem preferência automática se o módulo, o auxiliar e o lançador forem protegidos e compatíveis com o runtime nativo. O auxiliar e o lançador também devem corresponder ao mesmo ELF por hardlink. Se o RPM for antigo ou incompleto, o aplicativo procura uma instalação nativa completa no usuário. Se ela ainda não existir, o botão de instalação por usuário continua disponível.

Módulo e auxiliar carregam um identificador de compatibilidade do runtime nativo. A seleção apenas lê os arquivos; não executa o auxiliar nem carrega a biblioteca para descobrir sua versão. O identificador não é assinatura criptográfica nem prova de origem: as verificações de proprietário, permissões, arquivos regulares e diretórios continuam obrigatórias.

O instalador valida e lê os dois artefatos antes de substituir os binários instalados. Um pacote com componente antigo ou incompatível é rejeitado. O empacotador também rejeita esse conjunto antes de sobrescrever um pacote existente. O atalho reconhecido do próprio Axia que ainda aponta ao RPM obsoleto pode ser atualizado; atalhos de outros aplicativos são preservados.

## Perfis já configurados

A ação explícita em **Configurar navegadores** pode substituir o registro exato do módulo do RPM antigo pela instalação nativa do usuário. Isso inclui o registro que ficou no perfil depois de remover o RPM. O módulo de destino precisa ser a instalação nativa completa do próprio usuário. O caso inverso, usuário para RPM nativo atual, continua disponível.

A troca conserva backup privado e restaura a configuração anterior se o NSS não carregar a nova biblioteca. Repetir a configuração não duplica o registro. Um módulo de outro aplicativo ou um RPM atual não é substituído por essa regra de migração do legado.

## Atualização dos binários

Os binários anteriores a esta correção não contêm o novo identificador. Para testar a correção, compile e reinstale **módulo e auxiliar juntos**, pelo instalador da build atual ou por um RPM reconstruído. Não misture um executável novo com a biblioteca de um pacote anterior. Cadastro, carteira, preferências e identidade não precisam ser apagados nem recriados.

```bash
sh scripts/build_fedora.sh
./dist/fedora/axia-remotedid install-user --target dist/fedora
```

Execute a instalação como o usuário normal do Fedora. Feche os navegadores antes de configurar os perfis pela interface. Esta implementação não publica nem instala uma release automaticamente.

## Validação

As regressões em `installation/runtime/tests.rs` e `runtime_install_tests.rs` cobrem o RPM antigo, componentes misturados, precedência do RPM atual, ausência de instalação nativa, symlinks/permissões, reparo do próprio atalho e preservação da instalação após rejeitar pacote misturado.

`cargo xtask firefox-nss-smoke` exercita em perfil sintético a migração RPM antigo/usuário nativo, falha com rollback, backup privado e idempotência. `cargo xtask pkcs11-smoke` verifica ABI e assinatura sintética por OpenSC. O inventário explícito de laboratório continua funcionando sem exigir um auxiliar de assinatura instalado. Nenhum desses testes precisa de conta, cadastro ou assinatura reais.
