namespace cartservice.services;

public class CartServiceImpl : CartService.CartServiceBase
{
    public override Task<Cart> GetCart(GetCartRequest request, ServerCallContext context)
    {
        return Task.FromResult(new Cart());
    }
}
