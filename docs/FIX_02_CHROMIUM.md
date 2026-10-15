# Correção 2 — Chromium em execução

Implementada em 15/10/2026. Escopo: reconhecer os navegadores Chromium suportados em execução e reduzir a janela entre a verificação e a alteração da base NSS compartilhada. Os pontos 3 a 6 da revisão continuam pendentes.

## Comportamento

A descoberta e a proteção de configuração usam o mesmo catálogo de comandos para Chromium, Google Chrome, Brave, Microsoft Edge, Vivaldi e Opera. Isso inclui `google-chrome-stable`, `brave-browser-stable`, `microsoft-edge-stable` e `vivaldi-stable`, além dos nomes já reconhecidos.

Para processos do usuário atual, a verificação consulta tanto o executável real em `/proc/<pid>/exe` como o primeiro argumento em `/proc/<pid>/cmdline`. Os nomes internos `chrome`, `brave`, `msedge` e `vivaldi-bin` complementam os comandos de entrada. Caminhos completos e o sufixo do kernel ` (deleted)` no executável são tratados sem executar os arquivos. Somente nomes exatos são reconhecidos: um argumento que mencione um navegador não transforma outro programa em navegador.

O motivo para consultar as duas fontes está no [wrapper oficial do Chromium](https://chromium.googlesource.com/chromium/src/+/HEAD/chrome/installer/linux/common/wrapper): ele usa `exec -a` e pode preservar o nome do lançador no primeiro argumento. Processos que terminaram durante a consulta são ignorados; se um processo ainda presente não permite ler nenhuma das duas fontes, a configuração é interrompida. Nenhuma linha de comando é exibida ou salva.

## Ordem de proteção e recuperação

1. Verifica os processos antes de criar o diretório da base, alterar suas permissões ou criar o arquivo de controle do Axia.
2. Mantém a trava entre instâncias Axia e prepara o backup privado quando necessário.
3. Verifica novamente imediatamente antes de `NSS_InitReadWrite`, que também pode criar arquivos. Uma recusa nesse ponto não abre NSS nem restaura o backup sobre uma base potencialmente em uso.
4. Depois de abrir NSS, verifica novamente antes de registrar ou substituir o módulo. A criação de uma base nova também verifica antes do ajuste final de permissões.
5. Em caso de falha, fecha NSS e só restaura o backup se uma nova verificação confirmar os navegadores fechados. Se houver navegador aberto ou erro na verificação, conserva o backup e informa que a restauração automática foi suspensa.

O Axia não encerra navegadores, remove travas deles nem tenta assumir seus perfis. A trava `.axia-integration.lock` coordena apenas o Axia. O [ProcessSingleton do Chromium](https://chromium.googlesource.com/chromium/src/+/HEAD/chrome/browser/process_singleton_posix.cc) usa arquivos e sockets por diretório de dados do navegador; isso não equivale a um bloqueio exclusivo da base NSS compartilhada entre todos os derivados. As verificações repetidas reduzem a janela de concorrência, mas não impedem uma abertura externa entre duas operações. Mantenha todos os navegadores dessa família fechados até concluir a configuração.

Firefox continua usando seu bloqueio de perfil `fcntl`, sem depender da lista de processos Chromium. Não houve instalação de navegadores nem ampliação de suporte a Flatpak.

## Validação automatizada

Os testes Rust de `browsers/chromium/tests.rs` cobrem todos os aliases do catálogo, executáveis internos, primeiro argumento diferente do executável, executável removido, argumentos vazios, nomes parecidos, outro usuário e processos encerrados ou impossíveis de inspecionar. Um teste cria processos `sleep` de laboratório com alias `google-chrome-stable` e executável chamado `chrome`, verificando o `/proc` real sem instalar ou abrir navegador.

Os testes de `browsers/browser_guard_tests.rs` verificam recusa antes de criar a base, preservação de arquivos e permissões quando a consulta falha, abertura de navegador simulada antes e durante a inicialização NSS e suspensão de rollback sobre uma base em uso. A inicialização NSS usa somente diretórios temporários; não carrega certificado de conta real.

```bash
cargo test --locked -p axia-desktop browsers::
sh scripts/test_rust_wsl.sh
```

Essa validação funciona no Fedora WSL, sem sessão de navegador. A validação manual dos navegadores reais no Fedora GNOME permanece para depois das seis correções, conforme combinado.
