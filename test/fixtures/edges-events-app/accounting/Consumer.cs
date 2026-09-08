using Confluent.Kafka;

public class Consumer
{
    private static readonly string TopicName = Environment.GetEnvironmentVariable("KAFKA_TOPIC") ?? "order-created";

    public async Task Run()
    {
        _consumer.Subscribe(TopicName);
        await eventBus.PublishAsync(new OrderPaidIntegrationEvent(id));
    }
}
