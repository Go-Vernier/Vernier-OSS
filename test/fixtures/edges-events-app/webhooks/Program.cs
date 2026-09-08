eventBus.AddSubscription<OrderPaidIntegrationEvent, OrderPaidIntegrationEventHandler>();

public class OrderPaidIntegrationEventHandler : IIntegrationEventHandler<OrderPaidIntegrationEvent>
{
}
