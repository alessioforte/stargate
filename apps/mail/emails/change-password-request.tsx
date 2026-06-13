import {
  Body,
  Button,
  Column,
  Container,
  Head,
  Heading,
  Html,
  Preview,
  Row,
  Section,
  Tailwind,
  Text,
} from "@react-email/components";

interface ChangePasswordRequestProps {
  token: string;
}

export const ChangePasswordRequest = ({
  token = "{{token}}",
}: ChangePasswordRequestProps) => {
  return (
    <Html>
      <Tailwind>
        <Head />
        <Body style={main}>
          <Preview>Change Password Request</Preview>
          <Container className="pt-10">
            <Section>
              <Row className="p-5">
                <Column>
                  <Heading className="text-[32px] font-bold text-center">
                    Change Password Request
                  </Heading>

                  <Text className="text-base">
                    We received a request to change your password.
                  </Text>
                  <Text className="text-base">
                    If you made this request, please click the button below to
                    proceed with changing your password.
                  </Text>

                  <Row className="my-20">
                    <Column className="text-center" colSpan={2}>
                      <Button
                        className="bg-[#007ee6] rounded border border-solid border-black/10 text-white font-bold cursor-pointer inline-block px-[30px] py-3 no-underline"
                        href={`https://stargate.so/change-password?token=${token}`}
                      >
                        Change Password
                      </Button>
                    </Column>
                  </Row>

                  <Text className="text-base -mt-[5px]">
                    If you didn't request this, just ignore and delete this
                    message. To keep your account secure, please don't forward
                    this email to anyone. Thank you!
                  </Text>
                </Column>
              </Row>
            </Section>
            <Text className="text-center text-xs leading-[24px] text-black/70">
              © {new Date().getFullYear()} | Stargate Inc. All rights reserved.
            </Text>
          </Container>
        </Body>
      </Tailwind>
    </Html>
  );
};

export default ChangePasswordRequest;

const main = {
  fontFamily:
    "-apple-system, BlinkMacSystemFont, 'Segoe UI', 'Roboto', 'Oxygen', 'Ubuntu', 'Cantarell', 'Fira Sans', 'Droid Sans', 'Helvetica Neue', sans-serif",
};
