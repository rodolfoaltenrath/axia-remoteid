# Correção 6 — resultados por navegador e perfil

Implementada sobre `b023562`, em 15/10/2026. Faz parte da alpha.2 em validação; não altera o RPM alpha.1 publicado.

## Comportamento do painel

**Configurar navegadores** continua exigindo seleção e confirmação explícitas. Cancelar ou não encontrar perfis retorna antes de selecionar o runtime ou abrir NSS. Cada alvo escolhido é tentado uma vez. Uma falha não interrompe a configuração dos demais. A base compartilhada dos navegadores Chromium continua sendo um único alvo.

O painel principal possui uma área própria de resultados, separada do cadastro e da carteira. Mostra navegador, perfil/localização e uma destas situações:

- **Configurado:** módulo carregado pelo NSS, quantidade de certificados e indicação de registro existente reutilizado, quando aplicável.
- **Ação necessária:** navegador aberto, base ocupada ou impossibilidade de verificar os processos. Mostra a orientação correspondente.
- **Falha:** erro local de configuração, incluindo a cadeia útil de causas NSS e o resultado da recuperação, quando existente.

O resumo distingue sucesso total, sucesso parcial, falha total, cancelamento e ausência de alvos. A instrução de reabrir navegadores aparece somente quando a tentativa configurou pelo menos um alvo. Resultados parciais identificam os alvos que precisam de correção e nova tentativa.

Uma nova tentativa substitui o resultado do mesmo alvo, identificado pela família e pelo caminho do perfil/base. Perfis com o mesmo nome continuam separados. Resultados de alvos não selecionados permanecem visíveis. Cancelar, não selecionar alvos ou encontrar erro de preparação não apaga os resultados anteriores. Uma tentativa que falha substitui o sucesso anterior daquele alvo, evitando apresentá-lo como recém-validado.

Os resultados existem **somente na memória, durante a sessão do painel**. Fechar e abrir o painel inicia uma nova lista. A tela informa essa duração: os registros apresentados são resultados das tentativas locais, não uma monitoração contínua do navegador. Não há arquivo de cache de configuração, nem mudança no cache de autorização de assinaturas.

## Erros e recuperação

A classificação usa tipos Rust, não a presença de palavras nas mensagens. Os motivos são `browser_open`, `profile_busy`, `inspection_unavailable` e `configuration`. Uma falha de permissão ao abrir a trava, por exemplo, não é automaticamente classificada como navegador aberto. A trava `fcntl` do Firefox e as verificações repetidas de processos Chromium permanecem ativas.

Quando há backup e falha, o relatório inclui sua localização privada e o resultado da recuperação:

| Estado | Significado |
| --- | --- |
| `not_needed` | Backup já criado, mas NSS ainda não foi aberto; nenhuma restauração foi necessária |
| `restored` | A restauração terminou com sucesso; o backup continua preservado |
| `suspended` | Chromium abriu ou a inspeção falhou; a base em uso não foi sobrescrita |
| `failed` | A restauração foi tentada e falhou; o backup continua preservado |

Restauração suspensa nunca recebe o texto de base restaurada. Causas da operação NSS e da recuperação continuam disponíveis. A rotina é exclusivamente local: não faz login, cadastro, autorização ou assinatura remota. O relatório contém metadados locais, caminhos e erros técnicos; não contém senhas, OTP, tokens ou corpos de resposta do serviço. Caminhos de perfis e backups devem ser removidos antes de compartilhar uma saída de diagnóstico publicamente.

## Contrato da CLI

`setup-browsers --list` conserva o inventário anterior, sem abrir NSS nem criar estado. `setup-firefox --yes` conserva o objeto `Registration` anterior, incluindo `--profile`, `--module` e `--home` para testes e diagnóstico.

**Mudança intencional na alpha.2:** `setup-browsers --yes` passa do antigo array de registros bem-sucedidos para um objeto de relatório versionado. Tenta todos os alvos selecionados pela família, imprime o relatório completo em stdout e retorna código **0 somente no sucesso total**. Falha total, sucesso parcial e ausência de alvos retornam **1**, preservando o JSON em stdout e uma orientação breve em stderr. Erros de infraestrutura anteriores ao lote, como runtime indisponível, continuam sendo erros em stderr e podem não produzir JSON.

Estrutura do relatório:

```json
{
  "schema_version": 1,
  "status": "failed",
  "results": [
    {
      "target": { "family": "firefox", "label": "Firefox — Trabalho", "database": "/perfil/sintetico", "create": false },
      "status": "failed",
      "registration": null,
      "failure": { "kind": "configuration", "message": "NSS: erro sintético", "recovery": null }
    }
  ],
  "notices": []
}
```

O exemplo mostra um lote com falha. Cada alvo configurado traz o objeto `registration` completo e `failure: null`; cada alvo sem sucesso traz `registration: null` e os detalhes em `failure`. Um lote parcial contém ambos os casos. `target` mantém os metadados opcionais de padrão Firefox e instalações da descoberta.

Valores do resumo: `configured`, `partial`, `failed`, `cancelled`, `no_targets`. Valores por alvo: `configured`, `needs_action`, `failed`. Cancelamento é produzido pelo fluxo gráfico, antes de qualquer registro; `--yes` não oferece cancelamento interativo.

## Validação automatizada

- Cinco regressões Rust novas cobrem lotes, preservação de sucessos após falha, deduplicação, estados distintos, classificação tipada e recuperação com causa NSS/backup.
- As regressões de proteção Chromium verificam também os tipos retornados antes de abrir NSS e quando a restauração fica suspensa. O teste entre processos mantém a trava Firefox real.
- O teste GTK existente chama a verificação do painel na mesma thread: resultados de perfis com nomes iguais, nova tentativa, cancelamento, ausência de alvos, erro de infraestrutura, restauração suspensa, preservação do estado da conta e descarte ao abrir novo painel. Usa Broadway e D-Bus privados.
- O teste CLI confirma JSON `no_targets`, código de falha e ausência de mutações, sem runtime instalado no HOME de laboratório.
- O teste Firefox/NSS acrescenta lotes reais contra duas bases sintéticas: falha total com restauração, falha do primeiro perfil seguida de sucesso do segundo, correção e repetição idempotente. Esse cenário usa o runtime nativo privado do laboratório e roda quando não há um RPM nativo que prevaleça sobre ele; os demais cenários NSS continuam ativos.

Os comandos e os totais finais estão no [plano de continuidade](CONTINUAR_CORRECOES_NAVEGADORES.md). Não houve acesso à conta real, alteração de perfil real, instalação de navegador, instalação do runtime no usuário nem publicação de release. A homologação manual no Fedora/GNOME continua pendente.
