# Continuidade do DesktopID Linux

Estado consolidado em 13/10/2026, após assinatura real por PKCS#11 e limpeza para publicação. Leia este arquivo, o [README](README.md), a [referência do protocolo](docs/PROTOCOL.md) e o código atual antes de propor alterações. Este documento substitui o antigo diário de investigação.

## Objetivo e resultados comprovados

O usuário utiliza certificado em nuvem, autorizado por senha e código do aplicativo Android Certisign (`com.certisign.certisign`). Quer migrar para Fedora com GNOME e usar Gerid, PJe, eproc, Projudi, PJeOffice, e-CAC, autenticação por certificado e assinatura de PDF. Instâncias/tribunais/versões ainda precisam ser identificados para os testes finais.

Não existe código-fonte C++ original recuperado. A solução é uma reimplementação Rust, não a recompilação do PKG. A análise dos instaladores macOS Intel e Windows recuperou o protocolo RemoteID. Não é necessário recomeçar a investigação ou instalar Qt/Ghidra para compilar.

O usuário confirmou, nesta ordem: login real; cadastro Linux com domínio `Local`; carteira com dois certificados RSA-2048; assinatura SHA-256 de texto sintético pelo CLI; finalmente assinatura real via módulo PKCS#11/OpenSC e janela local. Última mensagem confirmada: `Assinatura real via PKCS11/OpenSC aceita e verificada no Linux.` A assinatura foi verificada criptograficamente. Não há validação dos sites finais, Firefox, PJeOffice, PDF/PAdES ou GNOME físico ainda.

Após a reorganização passaram: 38 testes Rust, 19 Python, 14 verificações CLI e 12 PKCS#11/OpenSC, além de fmt/Clippy. O módulo release já tinha passado os mesmos 12 testes PKCS#11 antes da reorganização; não houve alteração funcional nele nesta etapa. O ZIP de fontes foi extraído em diretório temporário: publicação, 19 testes Python, metadados Cargo, sintaxe dos scripts e links internos foram verificados sem depender do arquivo local da pesquisa. GitHub Actions foi configurado para dados sintéticos em Fedora, sem conta, mas ainda não foi executado no GitHub.

## Código e ambiente

- Workspace Rust: cliente/auxiliar em `crates/desktopid-client`, módulo em `crates/desktopid-pkcs11`.
- Referência Python independente em `tests/reference`; fixtures públicos de laboratório nas crates. A chave privada sintética do fixture é intencional e nunca deve ser usada numa instalação real.
- Ambiente já validado: FedoraLinux-44 WSL2, usuário root, Rust/Cargo 1.98.1, OpenSSL 3, OpenSC, Zenity e WSLg. O mínimo Rust declarado 1.88 ainda não foi testado separadamente.
- Cache Cargo fora do repositório: `/root/.cache/desktopid-rust-target`. Identidade real fora do repositório: `/root/.config/desktopid-linux`. Biblioteca e auxiliar instalados em `/root/.local/lib/desktopid-linux` e `/root/.local/libexec/desktopid-linux`.
- No Fedora físico final, usar usuário normal, home e propriedade correspondentes. A identidade registrada deve ser transferida privadamente ou um cadastro novo decidido conscientemente. Não copiar segredos no Git.
- Fontes próprias sob Apache 2.0, escolhida pelo usuário. Instaladores proprietários, ferramentas portáteis, extrações, projetos Ghidra e automação de pesquisa obsoleta foram retirados dos fontes e arquivados em `.local-research/`, ignorado pelo Git. Exclusão física bloqueada pela revisão automática do ambiente; não insistir em contornar o bloqueio. A referência técnica conserva contratos, endereços e hashes dos originais. O diário antigo foi substituído por este documento.

## Cuidados que fazem parte do funcionamento

Nunca pedir senha/PIN/OTP/chaves/tokens pelo chat. Entradas reais devem acontecer no terminal ou janela local; testes automáticos usam chaves de laboratório e loopback. Não executar cadastro ou assinatura real só para verificar uma alteração de código. Solicitações reais devem corresponder a uma ação consciente do usuário; não repetir após erro ambíguo. Preservar chave, carteira e journals; não apagar marcadores para contornar a prevenção de recadastro.

Não registrar corpos de erro, nomes de titulares, valores de tokens ou referências pessoais em arquivos publicados. Estado privado usa Linux e permissões restritas. Não abaixar a política criptográfica do Fedora para habilitar SHA-1. Não prometer compatibilidade universal a partir de uma assinatura de laboratório.

Pontos do protocolo já resolvidos que não devem regredir: cadastro usa Bearer sem espaço; pedidos assinados usam espaço; domínio vazio vira `Local`; `urlAcesso` não integra a URL; `keyName` contém série primeiro e emissor segundo; CertiNext não é pré-condição RemoteID; ID inteiro 0 no pedido não implica ID numérico 0 na resposta. Verificação criptográfica é obrigatória.

O módulo anuncia autenticação protegida e aceita login local sem PIN; a autorização real ocorre no auxiliar na assinatura efetiva. Consultas de tamanho/buffer curto não pedem autorização e não assinam. Tokens de sessão não são persistidos ou reutilizados. Biblioteca não precisa ler a chave da instalação para enumerar certificados; apenas o auxiliar abre a identidade privada para os pedidos.

## Próximas etapas

1. Validar o pacote ou a compilação no Fedora físico com GNOME, primeiro com inventário local e dados sintéticos. A assinatura real PKCS#11 no WSL já passou; não repetir esse marco como pendência.
2. Integrar PJeOffice e Firefox nativos, um por vez: carregamento do módulo, enumeração, seleção de certificado, janela de autorização e operação final. Registrar versão e serviço/tribunal testado, sem dados pessoais. Não alterar registro global p11-kit ou perfis de navegador sem escopo definido.
3. Investigar requisitos reais dos serviços Gerid, PJe, eproc, Projudi e e-CAC. Autenticação TLS pode exigir RSA-PSS/TLS 1.3, atualmente não implementado; não confundir falha de integração com falha da assinatura remota.
4. Para PDF, integrar um assinador capaz de produzir/verificar o contêiner e validar cadeia, confiança, revogação e políticas. Assinatura de digest não é PDF/PAdES pronto.
5. Tratar Firefox Flatpak, empacotamento, atualização e eventual broker/cache como trabalhos próprios. TTL/cancelamento do servidor ainda desconhecidos; o cache original de 30 minutos é apenas do cliente.

Use os comandos do README para compilar, testar e empacotar. `scripts/check_publication.py` verifica conteúdo publicável, sem imprimir valores sensíveis. A limpeza não altera a implementação nem o estado da conta. Publicação no GitHub será feita pelo usuário; preparar fontes não autoriza enviar o projeto automaticamente.
