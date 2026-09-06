import demo_pb2_grpc


class EmailService(demo_pb2_grpc.EmailServiceServicer):
    pass


def start():
    demo_pb2_grpc.add_EmailServiceServicer_to_server(EmailService(), server)
