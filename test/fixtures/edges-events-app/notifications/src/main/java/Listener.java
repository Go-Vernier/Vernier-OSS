import org.springframework.amqp.rabbit.annotation.RabbitListener;
import org.springframework.kafka.annotation.KafkaListener;

public class Listener {
    @KafkaListener(topics = "order-created", groupId = "notifications")
    public void onOrder(String m) {}

    @RabbitListener(queues = Queues.queueName)
    public void onEmail(String m) {}
}
