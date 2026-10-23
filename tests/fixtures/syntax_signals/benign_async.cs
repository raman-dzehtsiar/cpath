using System.Threading.Tasks;
class Example
{
    async Task<int> ReadConstant()
    {
        return await Task.FromResult(42);
    }
}
