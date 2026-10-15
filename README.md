# Axia RemotedID

**Certificados digitais em nuvem no Linux.**

**[Baixar RPM — Fedora 44 x86_64](https://github.com/rodolfoaltenrath/axia-remoteid/releases/download/v0.1.0-alpha.1/axia-remotedid-0.1.0-0.1.alpha1.fc44.x86_64.rpm)** · **[Guia de instalação e testes](docs/TESTING.md)**

O Axia RemotedID conecta certificados em nuvem da Certisign a aplicações Linux compatíveis. Com uma interface gráfica, permite cadastrar o computador, consultar os certificados da conta e autorizar seu uso em autenticação e operações de assinatura digital.

A solução atende profissionais e organizações que precisam acessar serviços com certificado digital em um ambiente Linux. A versão atual está em **fase alpha**, com foco no Fedora e no Firefox nativo.

A próxima alpha traz uma interface nativa e configuração dos navegadores já instalados, sem exigir Python ou Zenity e sem instalar navegador junto. A versão pública para download continua sendo a **alpha.1**; as novidades da **alpha.2** estão em validação.

## Principais recursos

- **Conexão com a conta:** cadastro do computador e atualização dos certificados pela interface gráfica.
- **Integração com o Firefox:** disponibilização dos certificados para serviços que oferecem autenticação por certificado digital.
- **Detecção de navegadores na alpha.2:** preparação do Firefox e da base de certificados dos navegadores Chromium encontrados no computador, com escolha pelo usuário.
- **Autorização pelo usuário:** solicitação de senha e código temporário do aplicativo Certisign quando uma operação exige autorização.
- **Sessão temporária:** recurso de reutilização da autorização por até dez minutos no mesmo aplicativo e certificado, para reduzir solicitações repetidas.
- **Inicialização opcional:** escolha, na primeira abertura, de iniciar o aplicativo ao entrar na sessão do Fedora.

## Como funciona

Após a instalação, abra **Axia RemotedID Alpha** pelo menu de aplicativos e conecte sua conta. O computador será cadastrado uma vez, e os certificados disponíveis serão sincronizados.

Com a integração do Firefox configurada, acesse o serviço desejado e escolha a opção de entrada com certificado digital. O navegador apresenta os certificados disponíveis, e o Axia RemotedID solicita a autorização necessária. Etapas adicionais de autenticação, como o segundo fator do serviço acessado, seguem o fluxo desse serviço.

## Segurança e privacidade

A chave privada do certificado permanece no serviço em nuvem. Senhas e códigos temporários não são gravados em disco nem armazenados no cache de autorização.

A sessão temporária mantém apenas o token de autorização em memória, com prazo limitado e separado por aplicativo e certificado. Fechar o navegador ou encerrar a sessão do dispositivo elimina essa autorização local. O serviço em nuvem pode exigir uma nova autorização antes do prazo; a reutilização por dez minutos passou nos testes locais e ainda está em validação no acesso real.

A inicialização automática mantém o aplicativo na bandeja, sem abrir janela ou aparecer na dock. O menu permite abrir o aplicativo, consultar a quantidade de certificados salvos, alterar a inicialização e sair. A conexão com a conta e as autorizações dependem de uma ação do usuário. Esse comportamento está na build de desenvolvimento; o RPM alpha.1 publicado ainda exibe a mensagem de inicialização.

## Compatibilidade

| Ambiente ou recurso | Situação atual |
| --- | --- |
| Fedora 44 com GNOME | Interface gráfica, cadastro e consulta de certificados validados |
| Firefox nativo | Integração com os certificados disponível |
| Chromium nativo — alpha.2 | Autenticação TLS 1.2 validada em laboratório; uso real com Certisign ainda precisa de confirmação |
| GERID / INSS | Acesso real por certificado confirmado, incluindo o segundo fator do serviço |
| Chrome, Brave, Edge, Vivaldi e Opera — alpha.2 | Detecção implementada; cada distribuição ainda precisa de validação própria |
| Navegadores Flatpak e demais portais | Compatibilidade ainda não validada |
| Assinatura de documentos PDF | Ainda não disponível nesta versão |

O acesso confirmado ao GERID não implica compatibilidade com todos os serviços. A versão alpha destina-se a testes e avaliação; a integração deve ser verificada no ambiente de uso.

## Instalação e primeiros passos

Para usar o pacote já compilado no Fedora, consulte o [guia de instalação e testes](docs/TESTING.md). Não é necessário instalar ferramentas de desenvolvimento. Para compilar a partir do código, consulte o [guia técnico](docs/DEVELOPMENT.md).

1. Instale o aplicativo no Fedora e abra **Axia RemotedID Alpha**.
2. Escolha se deseja iniciar o aplicativo com a sessão do sistema.
3. Conecte sua conta Certisign e atualize os certificados.
4. Configure a integração com o Firefox conforme o guia e acesse o serviço desejado.

## Sobre o projeto

O Axia RemotedID é um projeto independente de código aberto, distribuído sob a [licença Apache 2.0](LICENSE). Não é um produto oficial da Certisign.

Para informações de instalação, contribuição e funcionamento interno, consulte o [guia técnico](docs/DEVELOPMENT.md) e a [referência do protocolo](docs/PROTOCOL.md).
