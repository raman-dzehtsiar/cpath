using System;
class Example
{
    Action Make(int input)
    {
        int value = input;
        return () =>
        {
            value += 1;
            value *= 2;
        };
    }
}
